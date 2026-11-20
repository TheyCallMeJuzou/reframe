//! What reframe asks of Windows itself, beyond what GPUI offers: windows hidden and shown, the
//! tray icon with its menu, one reframe per profile, being told when the system goes down, and
//! starting with the system.
//!
//! Takes the place of what Electron did for the old version (`Tray`, `requestSingleInstanceLock`,
//! `setLoginItemSettings`, the `query-session-end` event) and of its `src/main/win32.ts`.
//!
//! Everything here runs on the main thread. Calls that make Windows send messages to reframe's
//! windows at once (showing, hiding) must not be made from inside a GPUI update: GPUI's own
//! handlers for those messages would find the app in use. The callers post them instead.

use std::cell::RefCell;

use futures::channel::mpsc::UnboundedSender;
use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT,
    RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{CreateMutexW, SetProcessShutdownParameters};
use windows::Win32::UI::Shell::{
    ITaskbarList3, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
    TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, CreateIconFromResourceEx, CreateIconIndirect, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyMenu, FLASHW_TIMERNOFG, FLASHW_TRAY, FLASHWINFO, FindWindowW,
    FlashWindowEx, GetCursorPos, GetSystemMetrics, GetWindowThreadProcessId, HICON, HMENU, ICONINFO, IsIconic,
    IsWindowVisible, LR_DEFAULTCOLOR, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, PostMessageW,
    RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNA, SetForegroundWindow,
    ShowWindowAsync, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CONTEXTMENU, WM_ENDSESSION, WM_LBUTTONUP, WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP, WNDCLASSW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_PARENT, GetAncestor, GetClientRect, GetDesktopWindow, GetWindowRect, GetWindowTextW, IsWindow,
};
use windows::core::{HSTRING, PCWSTR, w};

/// The tray icon tells of the mouse with this message.
const WM_TRAY: u32 = WM_APP + 1;
/// A second reframe that was started tells the running one with this message.
const WM_REVEAL: u32 = WM_APP + 2;
/// The installer asks the running reframe to quit with this message, before it replaces it.
const WM_QUIT_REQUEST: u32 = WM_APP + 3;
/// The class of the window that takes these messages; its title is the profile's key.
const CONTROL_CLASS: PCWSTR = w!("reframe-native-control");

// ---------- windows ----------

/// The system's handle of a GPUI window.
pub fn hwnd_of(window: &Window) -> Option<isize> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

fn hwnd(handle: isize) -> HWND {
    HWND(handle as *mut core::ffi::c_void)
}

/// Out of sight (and off the taskbar). Not from inside a GPUI update.
pub fn hide(window: isize) {
    // SAFETY: a window handle, valid or not, is all this takes.
    unsafe {
        let _ = ShowWindowAsync(hwnd(window), SW_HIDE);
    }
}

/// On screen again, back from the taskbar too. `activate`: it also comes to the front (GPUI's
/// `activate_window` does the rest of that). Not from inside a GPUI update.
pub fn show(window: isize, activate: bool) {
    // SAFETY: as above.
    unsafe {
        let how = if IsIconic(hwnd(window)).as_bool() {
            SW_RESTORE
        } else if activate {
            SW_SHOW
        } else {
            SW_SHOWNA
        };
        let _ = ShowWindowAsync(hwnd(window), how);
    }
}

/// Whether the window is on screen (minimized counts: it is on the taskbar).
pub fn is_visible(window: isize) -> bool {
    // SAFETY: as above.
    unsafe { IsWindowVisible(hwnd(window)).as_bool() }
}

/// The window's taskbar button flashes until the window is looked at.
pub fn flash(window: isize) {
    let info = FLASHWINFO {
        cbSize: size_of::<FLASHWINFO>() as u32,
        hwnd: hwnd(window),
        dwFlags: FLASHW_TRAY | FLASHW_TIMERNOFG,
        uCount: 0,
        dwTimeout: 0,
    };
    // SAFETY: the structure is filled in and says how big it is.
    unsafe {
        let _ = FlashWindowEx(&info);
    }
}

// ---------- one reframe per profile ----------

/// A name for the profile that Windows takes in names of its own: no backslashes.
pub fn profile_key(user_data: &std::path::Path) -> String {
    // FNV-1a over the path as Windows compares it.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in user_data.to_string_lossy().to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("reframe-{hash:016x}")
}

/// Whether this is the only reframe for the profile. The claim lasts as long as the process.
pub fn claim_profile(key: &str) -> bool {
    let name = HSTRING::from(format!("Local\\{key}"));
    // SAFETY: the name is a valid string; the mutex is never closed, on purpose.
    unsafe {
        let mutex: windows::core::Result<HANDLE> = CreateMutexW(None, true, &name);
        mutex.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Tells the reframe that runs for the profile to show its windows, and lets it come to the
/// front. `build`: which build this one is (see [`TrayEvent::Reveal`]). False when it does not
/// answer (it is still starting, or on its way out).
pub fn reveal_running(key: &str, build: usize) -> bool {
    let title = HSTRING::from(key);
    // SAFETY: plain calls with valid strings.
    unsafe {
        let Ok(control) = FindWindowW(CONTROL_CLASS, &title) else {
            return false;
        };
        let mut process = 0;
        GetWindowThreadProcessId(control, Some(&mut process));
        let _ = AllowSetForegroundWindow(process);
        PostMessageW(Some(control), WM_REVEAL, WPARAM(build), LPARAM(0)).is_ok()
    }
}

/// Asks the reframe that runs for the profile to quit as the user would (its sessions end and
/// resume on the next start). False when none runs.
pub fn ask_running_to_quit(key: &str) -> bool {
    let title = HSTRING::from(key);
    // SAFETY: plain calls with valid strings.
    unsafe {
        let Ok(control) = FindWindowW(CONTROL_CLASS, &title) else {
            return false;
        };
        PostMessageW(Some(control), WM_QUIT_REQUEST, WPARAM(0), LPARAM(0)).is_ok()
    }
}

/// Gives a shortcut the id Windows knows reframe by, so that a taskbar button pinned from it is
/// the one of the running reframe, and its notifications show under its name. The installer has
/// reframe do this for the Start menu entry it makes.
pub fn set_shortcut_app_id(link: &std::path::Path, app_id: &str) -> windows::core::Result<()> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile, STGM_READWRITE,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PWSTR};

    // SAFETY: COM objects used as documented; the string the value points to outlives the call
    // that copies it, and the value is not cleared (it does not own the string).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shortcut: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let file: IPersistFile = shortcut.cast()?;
        file.Load(&HSTRING::from(link.as_os_str()), STGM_READWRITE)?;
        let store: IPropertyStore = shortcut.cast()?;
        let mut id: Vec<u16> = app_id.encode_utf16().chain(std::iter::once(0)).collect();
        let value = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        pwszVal: PWSTR(id.as_mut_ptr()),
                    },
                }),
            },
        };
        store.SetValue(&PKEY_AppUserModel_ID, &value)?;
        store.Commit()?;
        file.Save(PCWSTR::null(), true)
    }
}

/// reframe is told first when the system goes down, before the claudes it runs are closed
/// (which then must not count as ended by the user).
pub fn shut_down_first() {
    // SAFETY: plain call. Programs are at 0x280 unless they say otherwise; 0x3ff is the highest.
    unsafe {
        let _ = SetProcessShutdownParameters(0x3ff, 0);
    }
}

// ---------- the tray icon and the window that takes its messages ----------

/// What reaches reframe through its control window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// A click on the tray icon.
    Clicked,
    /// An entry of the tray menu was picked.
    Command(u32),
    /// reframe was started a second time: the windows are shown. `build` is 1 for a development
    /// build and 2 for an installed one.
    Reveal { build: usize },
    /// Windows shuts down, restarts or logs the user off.
    SystemEnding,
    /// The installer asks reframe to quit.
    Quit,
}

/// An entry of the tray menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEntry {
    Item {
        id: u32,
        label: String,
        /// With a check mark or without; `None` for an entry that has none.
        checked: Option<bool>,
        enabled: bool,
    },
    Separator,
    /// An entry that opens more entries.
    Submenu {
        label: String,
        children: Vec<TrayEntry>,
    },
}

/// Fills a native menu with entries (a submenu is a menu of its own, which its parent destroys).
///
/// # Safety
/// `menu` is a menu made by `CreatePopupMenu` that has not been destroyed.
unsafe fn fill_menu(menu: HMENU, entries: &[TrayEntry]) {
    for entry in entries {
        // SAFETY: as the caller promises; the strings outlive the calls.
        unsafe {
            match entry {
                TrayEntry::Separator => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                TrayEntry::Item {
                    id,
                    label,
                    checked,
                    enabled,
                } => {
                    let mut flags = MF_STRING;
                    if *checked == Some(true) {
                        flags |= MF_CHECKED;
                    }
                    if !enabled {
                        flags |= MF_GRAYED;
                    }
                    let label = wide(label);
                    let _ = AppendMenuW(menu, flags, *id as usize, PCWSTR(label.as_ptr()));
                }
                TrayEntry::Submenu { label, children } => {
                    let Ok(submenu) = CreatePopupMenu() else { continue };
                    fill_menu(submenu, children);
                    let label = wide(label);
                    let _ = AppendMenuW(menu, MF_STRING | MF_POPUP, submenu.0 as usize, PCWSTR(label.as_ptr()));
                }
            }
        }
    }
}

struct Control {
    window: HWND,
    sender: UnboundedSender<TrayEvent>,
    /// The icon in the tray, once there is one.
    icon: Option<HICON>,
    tooltip: String,
    menu: Vec<TrayEntry>,
    /// The message Explorer sends when it starts again: the icon has to be added anew.
    taskbar_created: u32,
}

thread_local! {
    static CONTROL: RefCell<Option<Control>> = const { RefCell::new(None) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn icon_data(control: &Control) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: control.window,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: control.icon.unwrap_or_default(),
        ..Default::default()
    };
    // The tooltip holds 127 characters and a zero.
    for (slot, unit) in data.szTip.iter_mut().zip(control.tooltip.encode_utf16().take(127)) {
        *slot = unit;
    }
    data
}

/// Creates the hidden window that takes the tray's messages, a second start's call and the
/// system's notice that it goes down. Its events arrive at `sender`. The window's handle, or
/// `None` if it could not be made.
pub fn create_control(key: &str, sender: UnboundedSender<TrayEvent>) -> Option<isize> {
    // SAFETY: the class and the window are made from valid strings and a procedure that lives
    // as long as the program.
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(control_proc),
            hInstance: instance.into(),
            lpszClassName: CONTROL_CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            CONTROL_CLASS,
            &HSTRING::from(key),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        CONTROL.with_borrow_mut(|control| {
            *control = Some(Control {
                window,
                sender,
                icon: None,
                tooltip: String::new(),
                menu: Vec::new(),
                taskbar_created,
            });
        });
        Some(window.0 as isize)
    }
}

/// Puts the icon into the tray. `small` and `large` are PNG files of 16 and 32 pixels. Whether
/// Windows took it.
pub fn add_tray_icon(small: &[u8], large: &[u8]) -> bool {
    CONTROL.with_borrow_mut(|control| {
        let Some(control) = control else { return false };
        // SAFETY: the bytes are a PNG file, which Windows reads as an icon.
        unsafe {
            let size = GetSystemMetrics(SM_CXSMICON);
            let png = if size > 16 { large } else { small };
            control.icon = CreateIconFromResourceEx(png, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR).ok();
            control.icon.is_some() && Shell_NotifyIconW(NIM_ADD, &icon_data(control)).as_bool()
        }
    })
}

/// The tray icon's tooltip and menu, as they are from now on.
pub fn set_tray(tooltip: &str, menu: Vec<TrayEntry>) {
    CONTROL.with_borrow_mut(|control| {
        let Some(control) = control else { return };
        control.menu = menu;
        if control.tooltip != tooltip {
            control.tooltip = tooltip.to_owned();
            if control.icon.is_some() {
                // SAFETY: the structure is filled in by `icon_data`.
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &icon_data(control));
                }
            }
        }
    });
}

/// The tray menu and tooltip as they are now (for tests, which cannot open the menu).
pub fn tray() -> (String, Vec<TrayEntry>) {
    CONTROL.with_borrow(|control| {
        control
            .as_ref()
            .map(|control| (control.tooltip.clone(), control.menu.clone()))
            .unwrap_or_default()
    })
}

/// Takes the icon out of the tray (when reframe quits; Windows would leave it there until the
/// pointer passes over it).
pub fn remove_tray_icon() {
    CONTROL.with_borrow_mut(|control| {
        let Some(control) = control else { return };
        if control.icon.take().is_some() {
            // SAFETY: as above.
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(control));
            }
        }
    });
}

fn send(event: TrayEvent) {
    CONTROL.with_borrow(|control| {
        if let Some(control) = control {
            let _ = control.sender.unbounded_send(event);
        }
    });
}

/// Shows the tray menu at the pointer and says what was picked.
fn show_tray_menu(window: HWND) {
    // The entries are copied out: while the menu is open, Windows goes on handing out messages,
    // and reframe may change the menu meanwhile.
    let entries = CONTROL.with_borrow(|control| control.as_ref().map(|control| control.menu.clone()));
    let Some(entries) = entries else { return };
    // SAFETY: the menu is made, shown and destroyed here; its strings outlive the calls.
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        fill_menu(menu, &entries);
        let mut pointer = POINT::default();
        let _ = GetCursorPos(&mut pointer);
        // Without this the menu does not close when the user clicks somewhere else.
        let _ = SetForegroundWindow(window);
        let picked = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            pointer.x,
            pointer.y,
            None,
            window,
            None,
        );
        let _ = PostMessageW(Some(window), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        if picked.0 > 0 {
            send(TrayEvent::Command(picked.0 as u32));
        }
    }
}

unsafe extern "system" fn control_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_TRAY => {
            // The low word says what the mouse did on the icon.
            match (lparam.0 as u32) & 0xffff {
                WM_LBUTTONUP => send(TrayEvent::Clicked),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_tray_menu(window),
                _ => {}
            }
            LRESULT(0)
        }
        WM_REVEAL => {
            send(TrayEvent::Reveal { build: wparam.0 });
            LRESULT(0)
        }
        WM_QUIT_REQUEST => {
            send(TrayEvent::Quit);
            LRESULT(0)
        }
        // Windows asks every window whether it may go down, then tells them that it does.
        WM_QUERYENDSESSION => {
            send(TrayEvent::SystemEnding);
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                send(TrayEvent::SystemEnding);
            }
            LRESULT(0)
        }
        _ => {
            let again = CONTROL.with_borrow(|control| {
                control
                    .as_ref()
                    .is_some_and(|control| control.taskbar_created == message && control.icon.is_some())
            });
            if again {
                CONTROL.with_borrow(|control| {
                    if let Some(control) = control {
                        // SAFETY: the structure is filled in by `icon_data`.
                        unsafe {
                            let _ = Shell_NotifyIconW(NIM_ADD, &icon_data(control));
                        }
                    }
                });
            }
            // SAFETY: what is not reframe's is Windows' to answer.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}

// ---------- the badge on a window's taskbar button ----------

thread_local! {
    /// The taskbar, and the badge's picture, once they were asked for.
    static TASKBAR: RefCell<Option<(ITaskbarList3, HICON)>> = const { RefCell::new(None) };
}

/// A small filled circle in a colour, as an icon.
fn dot_icon(red: u8, green: u8, blue: u8) -> Option<HICON> {
    const SIZE: usize = 16;
    let mut pixels = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - SIZE as f32 / 2.0, y as f32 + 0.5 - SIZE as f32 / 2.0);
            // Radius 6 with a soft edge.
            let alpha = (6.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            let at = (y * SIZE + x) * 4;
            // Blue, green, red, alpha, with the colours multiplied by alpha.
            pixels[at] = (f32::from(blue) * alpha).round() as u8;
            pixels[at + 1] = (f32::from(green) * alpha).round() as u8;
            pixels[at + 2] = (f32::from(red) * alpha).round() as u8;
            pixels[at + 3] = (255.0 * alpha).round() as u8;
        }
    }
    let mask = [0u8; SIZE * SIZE / 8];
    // SAFETY: both bitmaps are made from buffers of the size their dimensions say, and freed
    // once the icon has its own copies.
    unsafe {
        let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, Some(pixels.as_ptr().cast()));
        let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// Puts a badge on a window's taskbar button (a dot in the colour of a waiting session's
/// status), with what a screen reader says for it, or takes it off. Not from inside a GPUI
/// update: the taskbar is another program, and waiting for it lets messages in.
pub fn set_badge(window: isize, description: Option<&str>) {
    TASKBAR.with_borrow_mut(|taskbar| {
        if taskbar.is_none() {
            // SAFETY: COM is set up on this thread by GPUI; the object is asked for by its
            // well-known class.
            let list: Option<ITaskbarList3> =
                unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok() };
            *taskbar = list.zip(dot_icon(0xe0, 0xb0, 0x4a));
        }
        let Some((list, icon)) = taskbar else { return };
        // SAFETY: a window handle, an icon that lives as long as the program, and a string that
        // outlives the call.
        unsafe {
            let _ = match description {
                Some(description) => list.SetOverlayIcon(hwnd(window), *icon, &HSTRING::from(description)),
                None => list.SetOverlayIcon(hwnd(window), HICON::default(), PCWSTR::null()),
            };
        }
    });
}

// ---------- starting with the system ----------

/// Where Windows keeps what it starts when the user logs in.
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// A text value under one of the user's registry keys, if it is there.
pub fn registry_text(key_path: &str, name: &str) -> Option<String> {
    let mut key = HKEY::default();
    // SAFETY: the key is opened into a variable that outlives the call.
    let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key_path), None, KEY_READ, &mut key) };
    if opened.is_err() {
        return None;
    }
    let name = HSTRING::from(name);
    let mut bytes = 0u32;
    // SAFETY: first the size is asked for, then a buffer of that size is filled.
    unsafe {
        let sized = RegQueryValueExW(key, &name, None, None, None, Some(&mut bytes));
        let mut text = None;
        if sized.is_ok() && bytes > 0 {
            let mut buffer = vec![0u16; (bytes as usize).div_ceil(2)];
            let read = RegQueryValueExW(
                key,
                &name,
                None,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            );
            if read.is_ok() {
                let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
                text = Some(String::from_utf16_lossy(&buffer[..end]));
            }
        }
        let _ = RegCloseKey(key);
        text
    }
}

/// Sets a text value under one of the user's registry keys (which is made if it is not
/// there), or (`None`) takes the value away.
pub fn set_registry_text(key_path: &str, name: &str, text: Option<&str>) -> Result<(), String> {
    let mut key = HKEY::default();
    // SAFETY: the key is opened into a variable that outlives the call.
    let opened = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key_path),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    opened.ok().map_err(|error| error.to_string())?;
    let name = HSTRING::from(name);
    // SAFETY: the value is a string with its terminating zero, given with its size in bytes.
    let result = unsafe {
        let result = match text {
            Some(text) => {
                let text = wide(text);
                let bytes = std::slice::from_raw_parts(text.as_ptr().cast::<u8>(), text.len() * 2);
                RegSetValueExW(key, &name, None, REG_SZ, Some(bytes))
            }
            None => match RegDeleteValueW(key, &name) {
                // Not there: as asked.
                error if error == ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            },
        };
        let _ = RegCloseKey(key);
        result
    };
    result.ok().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use windows::Win32::System::Registry::RegDeleteKeyW;

    use super::*;

    #[test]
    fn a_registry_text_is_set_read_and_taken_away() {
        // A key of its own, not the one Windows starts programs from.
        let key = format!("Software\\reframe-native-test-{}", std::process::id());
        assert_eq!(registry_text(&key, "reframe"), None);
        let command = "\"C:\\Program Files\\reframe\\reframe.exe\" --autostart";
        set_registry_text(&key, "reframe", Some(command)).unwrap();
        assert_eq!(registry_text(&key, "reframe").as_deref(), Some(command));
        assert_eq!(registry_text(&key, "other"), None);
        set_registry_text(&key, "reframe", None).unwrap();
        assert_eq!(registry_text(&key, "reframe"), None);
        // Taking away what is not there is no error.
        set_registry_text(&key, "reframe", None).unwrap();
        // SAFETY: deletes the key this test made.
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key));
        }
    }

    #[test]
    fn a_profile_has_one_key_however_its_path_is_written() {
        let key = profile_key(std::path::Path::new("C:\\Users\\Ann\\AppData\\Roaming\\reframe"));
        assert_eq!(
            key,
            profile_key(std::path::Path::new("c:\\users\\ann\\appdata\\roaming\\REFRAME"))
        );
        assert_ne!(key, profile_key(std::path::Path::new("D:\\portable\\data")));
        assert!(key.starts_with("reframe-") && !key.contains('\\'));
    }
}

// ---------- other programs' windows ----------

/// What other programs' windows do, as far as terminal windows dragged onto reframe go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    /// A window's move (or resize) by its title bar began or ended.
    MoveStarted(isize),
    MoveEnded(isize),
    /// A top-level window was made, or shown.
    Created(isize),
    Shown(isize),
}

thread_local! {
    static WINDOW_EVENTS: RefCell<Option<UnboundedSender<WindowEvent>>> = const { RefCell::new(None) };
}

unsafe extern "system" fn on_window_event(
    _hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, OBJID_WINDOW,
    };
    // Only windows themselves, not what is in them.
    if object != OBJID_WINDOW.0 || child != 0 || window.is_invalid() {
        return;
    }
    let handle = window.0 as isize;
    let told = match event {
        EVENT_SYSTEM_MOVESIZESTART => WindowEvent::MoveStarted(handle),
        EVENT_SYSTEM_MOVESIZEEND => WindowEvent::MoveEnded(handle),
        EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW => {
            // Top-level windows only.
            // SAFETY: asking about a window by handle.
            let parent = unsafe { GetAncestor(window, GA_PARENT) };
            // SAFETY: as above.
            let desktop = unsafe { GetDesktopWindow() };
            if parent != desktop {
                return;
            }
            if event == EVENT_OBJECT_CREATE {
                WindowEvent::Created(handle)
            } else {
                WindowEvent::Shown(handle)
            }
        }
        _ => return,
    };
    WINDOW_EVENTS.with_borrow(|sender| {
        if let Some(sender) = sender {
            let _ = sender.unbounded_send(told);
        }
    });
}

/// Follows other programs' windows: moves by the title bar, new top-level windows. Call on the
/// main thread (its message loop delivers the events). Whether that works.
pub fn watch_windows(sender: UnboundedSender<WindowEvent>) -> bool {
    use windows::Win32::UI::Accessibility::SetWinEventHook;
    use windows::Win32::UI::WindowsAndMessaging::{
        EVENT_OBJECT_CREATE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART,
        WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };
    WINDOW_EVENTS.set(Some(sender));
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // SAFETY: the callback lives as long as the program; the hooks end with it.
    unsafe {
        let moves = SetWinEventHook(
            EVENT_SYSTEM_MOVESIZESTART,
            EVENT_SYSTEM_MOVESIZEEND,
            None,
            Some(on_window_event),
            0,
            0,
            flags,
        );
        // The range includes EVENT_OBJECT_DESTROY, which is skipped like every other event.
        let made = SetWinEventHook(
            EVENT_OBJECT_CREATE,
            EVENT_OBJECT_SHOW,
            None,
            Some(on_window_event),
            0,
            0,
            flags,
        );
        !moves.is_invalid() && !made.is_invalid()
    }
}

/// Where a window is on the screen, in pixels: x, y, width, height.
pub fn window_rect(window: isize) -> Option<(i32, i32, i32, i32)> {
    let mut rect = RECT::default();
    // SAFETY: asking about a window by handle.
    unsafe { GetWindowRect(hwnd(window), &mut rect) }.ok()?;
    Some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}

/// Where a window's content is on the screen, in pixels: x, y, width, height.
pub fn client_rect(window: isize) -> Option<(i32, i32, i32, i32)> {
    let mut rect = RECT::default();
    // SAFETY: asking about a window by handle.
    unsafe { GetClientRect(hwnd(window), &mut rect) }.ok()?;
    let mut origin = POINT::default();
    // SAFETY: as above.
    if !unsafe { ClientToScreen(hwnd(window), &mut origin) }.as_bool() {
        return None;
    }
    Some((origin.x, origin.y, rect.right - rect.left, rect.bottom - rect.top))
}

/// The process a window belongs to.
pub fn window_process(window: isize) -> u32 {
    let mut pid = 0;
    // SAFETY: asking about a window by handle.
    unsafe { GetWindowThreadProcessId(hwnd(window), Some(&mut pid)) };
    pid
}

pub fn window_title(window: isize) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer is as long as it says.
    let length = unsafe { GetWindowTextW(hwnd(window), &mut buffer) };
    String::from_utf16_lossy(&buffer[..usize::try_from(length).unwrap_or(0)])
}

pub fn is_window(window: isize) -> bool {
    // SAFETY: asking about a window by handle.
    unsafe { IsWindow(Some(hwnd(window))) }.as_bool()
}

/// Where the pointer is on the screen, in pixels.
/// A message box with an error, for when reframe cannot start (there is no window yet).
pub fn error_box(title: &str, message: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW};
    // SAFETY: valid strings; the box has no owner.
    unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(message),
            &HSTRING::from(title),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

/// Where a top-level window is among all of them, from the front (0) back; windows not found
/// come last.
pub fn stacking_place(window: isize) -> usize {
    use windows::Win32::UI::WindowsAndMessaging::{GW_HWNDNEXT, GetTopWindow, GetWindow};
    // SAFETY: plain calls; the walk ends at the last window (or after many).
    unsafe {
        let mut next = GetTopWindow(None).ok();
        let mut place = 0;
        while let Some(current) = next {
            if current.0 as isize == window {
                return place;
            }
            place += 1;
            if place > 100_000 {
                break;
            }
            next = GetWindow(current, GW_HWNDNEXT).ok();
        }
        usize::MAX
    }
}

/// Puts a window where another one is, with its size, in front of it (for tests).
pub fn place_like(window: isize, model: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{HWND_TOP, SWP_NOACTIVATE, SetWindowPos};
    let Some((left, top, width, height)) = window_rect(model) else {
        return false;
    };
    // SAFETY: plain call on a window of reframe's.
    unsafe { SetWindowPos(hwnd(window), Some(HWND_TOP), left, top, width, height, SWP_NOACTIVATE).is_ok() }
}

/// The window in front (0 when there is none).
pub fn foreground() -> isize {
    // SAFETY: plain call.
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize }
}

pub fn cursor() -> (i32, i32) {
    let mut point = POINT::default();
    // SAFETY: plain values out.
    let _ = unsafe { GetCursorPos(&mut point) };
    (point.x, point.y)
}
