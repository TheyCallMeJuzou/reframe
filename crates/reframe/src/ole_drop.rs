//! Test hook (REFRAME_E2E): a real OLE drag, started by reframe itself and let go over one of its
//! windows, so that the tests go through the same drop target a drag from another program does
//! (GPUI's, with reframe's changes). It carries files that exist only in the drag, as a picture
//! dragged out of a browser does, or nothing the system knows, as a Windows Terminal tab does.
//!
//! The pointer is moved to where the drag goes (no button is pressed) and back afterwards. The
//! window is put in front of all others for the drag, and the drag only starts when nothing but
//! that window is under its whole path: it never lets go over another program.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HGLOBAL, POINT};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, DoDragDrop, IDropSource, IDropSource_Impl,
    OleInitialize,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Shell::{FD_FILESIZE, FILEDESCRIPTORW, FILEGROUPDESCRIPTORW, SHCreateDataObject};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GUITHREADINFO, GetAncestor, GetCursorPos, GetGUIThreadInfo, HWND_NOTOPMOST, HWND_TOPMOST, PostMessageW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursorPos, SetWindowPos, WM_MOUSEMOVE, WindowFromPoint,
};
use windows::core::{BOOL, HRESULT, PCWSTR, implement, w};

const DRAGDROP_S_DROP: HRESULT = HRESULT(0x0004_0100);
const DRAGDROP_S_USEDEFAULTCURSORS: HRESULT = HRESULT(0x0004_0102);

/// Lets go once the pointer has gone its way (`arrived`).
#[implement(IDropSource)]
struct Source {
    arrived: Arc<AtomicBool>,
}

#[allow(non_snake_case)]
impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, _escape: BOOL, _keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if self.arrived.load(Ordering::Acquire) {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn global(bytes: &[u8]) -> Result<HGLOBAL, String> {
    // SAFETY: the block is as big as what is copied into it.
    unsafe {
        let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|error| error.to_string())?;
        let target = GlobalLock(global) as *mut u8;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        let _ = GlobalUnlock(global);
        Ok(global)
    }
}

fn set(data: &IDataObject, format: u16, index: i32, bytes: &[u8]) -> Result<(), String> {
    let etc = FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let medium = STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 {
            hGlobal: global(bytes)?,
        },
        pUnkForRelease: std::mem::ManuallyDrop::new(None),
    };
    // SAFETY: the data object takes the memory over (`true`).
    unsafe { data.SetData(&etc, &medium, true) }.map_err(|error| error.to_string())
}

fn format(name: PCWSTR) -> u16 {
    // SAFETY: a valid string.
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

/// The drag's data: the files as "FileGroupDescriptorW" and "FileContents", or without files a
/// format nobody knows.
fn data_object(files: &[(String, Vec<u8>)]) -> Result<IDataObject, String> {
    // SAFETY: the shell's data object, made empty.
    let data: IDataObject = unsafe { SHCreateDataObject(None, None, None) }.map_err(|error| error.to_string())?;
    if files.is_empty() {
        set(&data, format(w!("reframe test drag")), -1, b"tab")?;
        return Ok(data);
    }
    let mut descriptors = Vec::new();
    for (name, contents) in files {
        // The structure is packed: its name is filled in apart and put in whole.
        let mut file_name = [0u16; 260];
        for (slot, unit) in file_name.iter_mut().zip(name.encode_utf16().take(259)) {
            *slot = unit;
        }
        descriptors.push(FILEDESCRIPTORW {
            dwFlags: FD_FILESIZE.0 as u32,
            nFileSizeLow: contents.len() as u32,
            cFileName: file_name,
            ..Default::default()
        });
    }
    // FILEGROUPDESCRIPTORW: the count, then the descriptors.
    let mut group = (descriptors.len() as u32).to_le_bytes().to_vec();
    for descriptor in &descriptors {
        // SAFETY: a plain structure, read as its bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (descriptor as *const FILEDESCRIPTORW).cast::<u8>(),
                size_of::<FILEDESCRIPTORW>(),
            )
        };
        group.extend_from_slice(bytes);
    }
    debug_assert!(group.len() >= size_of::<FILEGROUPDESCRIPTORW>());
    set(&data, format(w!("FileGroupDescriptorW")), -1, &group)?;
    let contents = format(w!("FileContents"));
    for (index, (_, bytes)) in files.iter().enumerate() {
        set(&data, contents, index as i32, bytes)?;
    }
    Ok(data)
}

/// Whether `window` is what is under every point of `path` (screen pixels).
fn only_over(window: HWND, path: &[(i32, i32)]) -> bool {
    path.iter().all(|&(x, y)| {
        // SAFETY: plain calls.
        unsafe {
            let under = WindowFromPoint(POINT { x, y });
            !under.is_invalid() && GetAncestor(under, GA_ROOT) == window
        }
    })
}

/// Drags `files` (or, without files, something no program knows) along `path` (screen pixels)
/// over `window` and lets go at its end; the pointer goes back where it was. The drag runs on a
/// thread of its own, as one from another program does, so reframe goes on meanwhile. `done`
/// gets whether the drag happened.
pub fn drag(
    window: isize,
    files: Vec<(String, Vec<u8>)>,
    path: Vec<(i32, i32)>,
    cx: &mut gpui_kit::App,
    done: impl FnOnce(bool) + 'static,
) {
    let window_value = window;
    let keep_on_top = move |on_top: bool| {
        let place = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: plain call on reframe's own window.
        unsafe {
            let _ = SetWindowPos(
                HWND(window_value as *mut _),
                Some(place),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    };
    keep_on_top(true);
    let (finished, finish) = futures::channel::oneshot::channel::<bool>();
    cx.spawn(async move |_| {
        done(finish.await.unwrap_or(false));
    })
    .detach();
    std::thread::spawn(move || {
        let window = HWND(window_value as *mut _);
        let done = move |dragged: bool| {
            let _ = finished.send(dragged);
        };
        std::thread::sleep(std::time::Duration::from_millis(150));
        if path.is_empty() || !only_over(window, &path) {
            keep_on_top(false);
            return done(false);
        }
        // SAFETY: OLE is set up for this thread; the drag uses objects that live through it.
        unsafe {
            let _ = OleInitialize(None);
            let mut before = POINT::default();
            let _ = GetCursorPos(&mut before);
            let Ok(data) = data_object(&files) else {
                keep_on_top(false);
                return done(false);
            };
            // The drag's loop goes on when the pointer moves: it is moved along the path from
            // another thread (a few moves per point, so the target hears of each), and nudged at
            // the end until the drag is over.
            let arrived = Arc::new(AtomicBool::new(false));
            let over = Arc::new(AtomicBool::new(false));
            let start = path[0];
            let drag_thread = GetCurrentThreadId();
            let mover = {
                let (arrived, over) = (arrived.clone(), over.clone());
                std::thread::spawn(move || {
                    // Without a pressed button the drag's mouse capture does not get the moves:
                    // each one is also told to the window that has it.
                    let move_to = |x: i32, y: i32| {
                        let _ = SetCursorPos(x, y);
                        let mut info = GUITHREADINFO {
                            cbSize: size_of::<GUITHREADINFO>() as u32,
                            ..Default::default()
                        };
                        if GetGUIThreadInfo(drag_thread, &mut info).is_ok() && !info.hwndCapture.is_invalid() {
                            // Where the pointer is, in the capturing window's own coordinates.
                            let mut at = POINT { x, y };
                            let _ = ScreenToClient(info.hwndCapture, &mut at);
                            let position = ((at.y as u16 as isize) << 16) | (at.x as u16 as isize);
                            let _ =
                                PostMessageW(Some(info.hwndCapture), WM_MOUSEMOVE, WPARAM(0x0001), LPARAM(position));
                        }
                        std::thread::sleep(std::time::Duration::from_millis(30));
                    };
                    for &(x, y) in &path {
                        for nudge in [0, 1, 0] {
                            move_to(x + nudge, y);
                        }
                    }
                    arrived.store(true, Ordering::Release);
                    let &(x, y) = path.last().unwrap_or(&(0, 0));
                    for nudge in [1, 0].into_iter().cycle().take(100) {
                        if over.load(Ordering::Acquire) {
                            break;
                        }
                        move_to(x + nudge, y);
                    }
                })
            };
            let _ = SetCursorPos(start.0, start.1);
            let source: IDropSource = Source { arrived }.into();
            let mut effect = DROPEFFECT(0);
            let _ = DoDragDrop(
                &data,
                &source,
                DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
                &mut effect,
            );
            over.store(true, Ordering::Release);
            let _ = mover.join();
            let _ = SetCursorPos(before.x, before.y);
        }
        keep_on_top(false);
        done(true);
    });
}
