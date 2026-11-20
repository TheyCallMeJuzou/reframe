//! Opening web links in the user's browser. The end-to-end tests catch them instead (as the
//! Electron version's tests replaced `shell.openExternal`): with the test channel on, a link is
//! remembered and no browser opens.

use gpui_kit::*;

/// Links opened while a test drives reframe, in order.
#[derive(Default)]
struct CaughtLinks(Vec<String>);

impl Global for CaughtLinks {}

/// Opens `url` in the browser, or (under test) remembers it.
pub fn open(url: &str, cx: &mut App) {
    if cx.has_global::<CaughtLinks>() {
        cx.global_mut::<CaughtLinks>().0.push(url.to_owned());
    } else {
        cx.open_url(url);
    }
}

/// From now on links are caught (the test channel calls this when it starts).
pub fn catch_links(cx: &mut App) {
    cx.set_global(CaughtLinks::default());
}

/// The links caught so far.
pub fn caught(cx: &App) -> Vec<String> {
    cx.try_global::<CaughtLinks>()
        .map(|caught| caught.0.clone())
        .unwrap_or_default()
}
