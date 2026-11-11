//! Copies the bundled ConPTY (conpty.dll + OpenConsole.exe, from the Windows Terminal project via
//! node-pty 1.1.0) next to the built executable, where alacritty_terminal looks for it. The one
//! built into Windows crashed the Electron version now and then when several sessions ended at
//! once.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=vendor/conpty");
    let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor/conpty");
    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out; the executable is three levels up.
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap())
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        std::fs::copy(source.join(name), target.join(name)).unwrap_or_else(|err| panic!("copying {name}: {err}"));
    }
}
