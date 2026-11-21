//! The clipboard as the terminals use it. The end-to-end tests put a stand-in in its place (as the
//! Electron version's tests did with `fakeClipboard`), so a test never changes the user's real
//! clipboard: pasting reads the stand-in, copying is caught.

use gpui_kit::*;

/// The stand-in: what pasting reads, and what was copied.
#[derive(Default)]
struct FakeClipboard {
    item: Option<ClipboardItem>,
    copied: Vec<String>,
}

impl Global for FakeClipboard {}

pub fn read(cx: &App) -> Option<ClipboardItem> {
    match cx.try_global::<FakeClipboard>() {
        Some(fake) => fake.item.clone(),
        None => cx.read_from_clipboard(),
    }
}

/// Files copied in a Linux file manager (see `platform::clipboard_files`); none from the
/// stand-in, whose files are its entries.
#[cfg(not(windows))]
pub fn copied_files(cx: &App) -> Vec<std::path::PathBuf> {
    if cx.has_global::<FakeClipboard>() {
        Vec::new()
    } else {
        crate::platform::clipboard_files()
    }
}

pub fn write(item: ClipboardItem, cx: &mut App) {
    if cx.has_global::<FakeClipboard>() {
        let text = item.text().unwrap_or_default();
        cx.global_mut::<FakeClipboard>().copied.push(text);
    } else {
        cx.write_to_clipboard(item);
    }
}

/// Test hook: from now on the stand-in holds `item` (`None`: nothing).
pub fn fake(item: Option<ClipboardItem>, cx: &mut App) {
    if !cx.has_global::<FakeClipboard>() {
        cx.set_global(FakeClipboard::default());
    }
    cx.global_mut::<FakeClipboard>().item = item;
}

/// Test hook: the texts copied since the stand-in took over.
pub fn copied(cx: &App) -> Vec<String> {
    cx.try_global::<FakeClipboard>()
        .map(|fake| fake.copied.clone())
        .unwrap_or_default()
}
