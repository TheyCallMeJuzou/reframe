//! reframe's additions to the drop target (see `WindowsDragDropHandler` in `window.rs`):
//!
//! - Files that exist only in the drag, such as a picture dragged out of a browser (offered as
//!   "FileGroupDescriptorW" with "FileContents" instead of files on disk), are taken: the window
//!   gets them as paths in a folder the app names, written when they are dropped.
//! - A drag of something that offers no standard data at all (a tab torn out of Windows
//!   Terminal) is not taken, but the app is told where it is, so it can show where it would go.
//!
//! Upstream takes nothing but files on disk and tells nothing about other drags.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::{HGLOBAL, HWND, S_OK};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED, TYMED_HGLOBAL, TYMED_ISTREAM};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_HDROP, CF_TEXT, CF_UNICODETEXT, ReleaseStgMedium};
use windows::Win32::UI::Shell::{FD_ATTRIBUTES, FD_FILESIZE, FILEDESCRIPTORW, FILEGROUPDESCRIPTORW};
use windows::core::{PCWSTR, w};

type ForeignDragHandler = Box<dyn Fn(isize, Option<(i32, i32)>)>;

thread_local! {
    static FOREIGN_DRAG: RefCell<Option<ForeignDragHandler>> = RefCell::new(None);
    static DROPPED_FILES_FOLDER: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Called (on the main thread) while something that is no file and no text is dragged over a
/// window: the window's handle and the pointer on the screen in pixels, then `None` once the drag
/// left the window or ended there.
pub fn set_foreign_drag_handler(handler: impl Fn(isize, Option<(i32, i32)>) + 'static) {
    FOREIGN_DRAG.with_borrow_mut(|slot| *slot = Some(Box::new(handler)));
}

/// Where files that exist only in a drag are written when they are dropped (each drop in a folder
/// of its own). The temp folder without it.
pub fn set_dropped_files_folder(folder: PathBuf) {
    DROPPED_FILES_FOLDER.with_borrow_mut(|slot| *slot = Some(folder));
}

pub(crate) fn report_foreign_drag(window: HWND, point: Option<(i32, i32)>) {
    FOREIGN_DRAG.with_borrow(|handler| {
        if let Some(handler) = handler {
            handler(window.0 as isize, point);
        }
    });
}

/// What a drag offers.
pub(crate) enum Offered {
    /// Files on disk.
    Files,
    /// Files that exist only in the drag.
    VirtualFiles(Vec<VirtualFile>),
    /// Text, a link, a picture's pixels: nothing for a window to take.
    Standard,
    /// Nothing the system knows (a Windows Terminal tab).
    Unknown,
}

/// A file that exists only in the drag.
#[derive(Clone)]
pub(crate) struct VirtualFile {
    /// Its place in the drag's list.
    index: i32,
    name: String,
    /// Its size, when the drag says.
    size: Option<u64>,
}

fn format(name: PCWSTR) -> u16 {
    // SAFETY: a valid string; the same name always gives the same number.
    unsafe { RegisterClipboardFormatW(name) as u16 }
}

fn format_etc(format: u16, index: i32, tymed: TYMED) -> FORMATETC {
    FORMATETC {
        cfFormat: format,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: index,
        tymed: tymed.0 as u32,
    }
}

fn offers(data: &IDataObject, format: u16) -> bool {
    // SAFETY: plain query with a filled-in structure.
    unsafe { data.QueryGetData(&format_etc(format, -1, TYMED(-1))) == S_OK }
}

pub(crate) fn offered(data: &IDataObject) -> Offered {
    if offers(data, CF_HDROP.0) {
        return Offered::Files;
    }
    let descriptor = format(w!("FileGroupDescriptorW"));
    if offers(data, descriptor) {
        let files = virtual_files(data, descriptor);
        if !files.is_empty() {
            return Offered::VirtualFiles(files);
        }
    }
    let standard = [
        CF_UNICODETEXT.0,
        CF_TEXT.0,
        CF_DIB.0,
        format(w!("UniformResourceLocatorW")),
        format(w!("UniformResourceLocator")),
        format(w!("HTML Format")),
        descriptor,
    ];
    if standard.into_iter().any(|format| offers(data, format)) {
        Offered::Standard
    } else {
        Offered::Unknown
    }
}

/// The files at the top of the drag's file list (folders and what is in them are left out).
fn virtual_files(data: &IDataObject, descriptor: u16) -> Vec<VirtualFile> {
    let mut files = Vec::new();
    // SAFETY: the medium is an HGLOBAL holding a FILEGROUPDESCRIPTORW, locked while it is read
    // and released after.
    unsafe {
        let Ok(mut medium) = data.GetData(&format_etc(descriptor, -1, TYMED_HGLOBAL)) else {
            return files;
        };
        let global = medium.u.hGlobal;
        let group = GlobalLock(global) as *const FILEGROUPDESCRIPTORW;
        if !group.is_null() {
            // The structures are packed: copied out before their fields are read.
            let count = std::ptr::read_unaligned(&raw const (*group).cItems) as usize;
            let first = (&raw const (*group).fgd) as *const FILEDESCRIPTORW;
            let room = GlobalSize(global).saturating_sub(size_of::<u32>()) / size_of::<FILEDESCRIPTORW>();
            for index in 0..count.min(room) {
                let file: FILEDESCRIPTORW = std::ptr::read_unaligned(first.add(index));
                let folder = file.dwFlags & FD_ATTRIBUTES.0 as u32 != 0
                    && file.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
                let file_name = file.cFileName;
                let end = file_name.iter().position(|unit| *unit == 0).unwrap_or(file_name.len());
                let name = String::from_utf16_lossy(&file_name[..end]);
                let size = (file.dwFlags & FD_FILESIZE.0 as u32 != 0)
                    .then(|| (u64::from(file.nFileSizeHigh) << 32) | u64::from(file.nFileSizeLow));
                if !folder && !name.is_empty() && !name.contains(['\\', '/']) {
                    files.push(VirtualFile {
                        index: index as i32,
                        name,
                        size,
                    });
                }
            }
            let _ = GlobalUnlock(global);
        }
        ReleaseStgMedium(&mut medium);
    }
    files
}

/// The largest file taken from a drag (as the Electron version of reframe had it).
const MAX_FILE: u64 = 200 * 1024 * 1024;

/// Where the files of a drag will be written: each in a new folder of its own in the dropped
/// files' folder, under a plain name (no folders, nothing Windows refuses in names, at most 120
/// characters).
pub(crate) fn planned_paths(files: &[VirtualFile]) -> Vec<PathBuf> {
    static DROPS: AtomicU64 = AtomicU64::new(0);
    let base = DROPPED_FILES_FOLDER
        .with_borrow(Clone::clone)
        .unwrap_or_else(|| std::env::temp_dir().join("dropped-files"));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    files
        .iter()
        .map(|file| {
            let name: String = file
                .name
                .chars()
                .map(|c| {
                    if c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                        '_'
                    } else {
                        c
                    }
                })
                .take(120)
                .collect();
            let name = name.trim_end_matches(['.', ' ']);
            let folder = base.join(format!("{stamp}-{}", DROPS.fetch_add(1, Ordering::Relaxed)));
            folder.join(if name.is_empty() { "dropped" } else { name })
        })
        .collect()
}

/// Writes the drag's files where [`planned_paths`] said; a file that cannot be had is skipped.
pub(crate) fn save_virtual_files(data: &IDataObject, files: &[VirtualFile], paths: &[PathBuf]) {
    let contents = format(w!("FileContents"));
    for (file, path) in files.iter().zip(paths) {
        if file.size.is_some_and(|size| size > MAX_FILE) {
            log::warn!("a dropped file of {:?} bytes is too big to take", file.size);
            continue;
        }
        if let Some(mut bytes) = file_contents(data, contents, file.index).filter(|bytes| bytes.len() as u64 <= MAX_FILE) {
            // A memory block can be bigger than the file in it.
            if let Some(size) = file.size.and_then(|size| usize::try_from(size).ok()) {
                bytes.truncate(size);
            }
            if let Some(folder) = path.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            if let Err(error) = std::fs::write(path, bytes) {
                log::warn!("could not write the dropped file {}: {error}", path.display());
            }
        }
    }
}

fn file_contents(data: &IDataObject, contents: u16, index: i32) -> Option<Vec<u8>> {
    // SAFETY: the medium is a stream or an HGLOBAL, as its `tymed` says; read, then released.
    unsafe {
        let mut medium = data
            .GetData(&format_etc(contents, index, TYMED(TYMED_ISTREAM.0 | TYMED_HGLOBAL.0)))
            .ok()?;
        let mut bytes = Vec::new();
        if medium.tymed == TYMED_ISTREAM.0 as u32 {
            if let Some(stream) = &*medium.u.pstm {
                let mut buffer = vec![0u8; 64 * 1024];
                loop {
                    let mut read = 0u32;
                    let result = stream.Read(buffer.as_mut_ptr().cast(), buffer.len() as u32, Some(&mut read));
                    if result.is_err() || read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read as usize]);
                }
            }
        } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
            bytes = global_bytes(medium.u.hGlobal);
        }
        ReleaseStgMedium(&mut medium);
        Some(bytes)
    }
}

/// # Safety
/// `global` is a valid global memory handle.
unsafe fn global_bytes(global: HGLOBAL) -> Vec<u8> {
    // SAFETY: as the caller promises; locked while it is copied.
    unsafe {
        let start = GlobalLock(global) as *const u8;
        if start.is_null() {
            return Vec::new();
        }
        let bytes = std::slice::from_raw_parts(start, GlobalSize(global)).to_vec();
        let _ = GlobalUnlock(global);
        bytes
    }
}
