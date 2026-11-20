//! Copies the bundled ConPTY (conpty.dll + OpenConsole.exe, from the Windows Terminal project via
//! node-pty 1.1.0) next to the built executable, where alacritty_terminal looks for it. The one
//! built into Windows crashed the Electron version now and then when several sessions ended at
//! once.
//!
//! Also gives the executable its icon (which the installer's shortcuts and uninstall entry show)
//! and its version information.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=vendor/conpty");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = manifest.join("vendor/conpty");
    // OUT_DIR is target/<profile>/build/<crate>-<hash>/out (and in Cargo's newer layout
    // target/<profile>/build/<crate>/<hash>/out): the executable is in the folder `build` is in.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let target = out
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "build"))
        .and_then(Path::parent)
        .unwrap_or_else(|| out.ancestors().nth(3).unwrap())
        .to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        std::fs::copy(source.join(name), target.join(name)).unwrap_or_else(|err| panic!("copying {name}: {err}"));
    }
    resources(&manifest, &out);
}

/// The icon and version resources, written for this version and compiled into the executable.
fn resources(manifest: &Path, out: &Path) {
    println!("cargo:rerun-if-changed=assets/reframe.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let numbers: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let [major, minor, patch] = [numbers[0], numbers[1], numbers[2]];
    let icon = manifest.join("assets").join("reframe.ico");
    let icon = icon.to_string_lossy().replace('\\', "\\\\");
    let script = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "reframe Team"
      VALUE "FileDescription", "reframe"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "reframe"
      VALUE "LegalCopyright", "Copyright (c) 2026 reframe Team"
      VALUE "OriginalFilename", "reframe.exe"
      VALUE "ProductName", "reframe"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let file = out.join("reframe.rc");
    std::fs::write(&file, script).unwrap();
    embed_resource::compile(&file, embed_resource::NONE)
        .manifest_optional()
        .unwrap_or_else(|err| panic!("compiling the resources: {err}"));
}
