//! Updates from reframe's releases on GitHub. An installed copy checks half a minute after the
//! start and then every six hours, downloads the installer in the background, checks it by its
//! SHA-512, and installs it when reframe quits, or right away on "Restart to update" (sessions
//! end the usual way first and resume after the restart). An update never quits or restarts
//! reframe by itself: installing stays the user's call. A portable copy cannot replace itself (the
//! installer would make a second, installed copy): it only tells when there is a newer version.
//!
//! Takes the place of the Electron version's `src/main/updates.ts` and of electron-updater, with
//! the same feed (see [`reframe_core::updates`]), so 0.x copies and this one update alike.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use base64::Engine;
use reframe_core::log::Logger;
#[cfg(windows)]
use reframe_core::updates::installer_args;
use reframe_core::updates::{download_url, file_name_of, is_newer, latest_tag, parse_latest_yml};
use serde_json::json;
use sha2::{Digest, Sha512};

/// What this copy does about updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMode {
    /// Nothing (a development build, a release build that was not installed).
    None,
    /// Downloads them and installs them on quit (an installed copy).
    Install,
    /// Tells about a newer version, to be downloaded by hand (the portable copy).
    Notify,
}

/// A downloaded update, waiting to be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ready {
    pub version: String,
    pub installer: PathBuf,
}

/// What a check found.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    Nothing,
    /// A newer version, for the portable copy to download by hand.
    Available(String),
    /// A newer version, downloaded.
    Ready(Ready),
}

/// Where to look and where to keep downloads.
#[derive(Clone)]
pub struct Feed {
    /// `{feed}/latest` is the newest release (see [`reframe_core::updates::FEED`]).
    pub url: String,
    /// Where downloads wait to be installed.
    pub cache: PathBuf,
    /// This copy's version.
    pub current: String,
}

/// The largest installer taken.
const MAX_INSTALLER: u64 = 1024 * 1024 * 1024;

/// What a release says about its update, as electron-builder named it: the Windows installer's,
/// or the Linux AppImage's (0.x copies read the same files).
pub const LATEST_YML: &str = if cfg!(windows) {
    "latest.yml"
} else {
    "latest-linux.yml"
};

/// One check: the newest release, whether it is newer than this copy, and for an installed copy
/// its installer, downloaded and checked. Blocking; `have` is the version already downloaded.
pub fn check(feed: &Feed, mode: UpdateMode, have: Option<&str>, log: &Logger) -> Result<Found, String> {
    let latest = format!("{}/latest", feed.url.trim_end_matches('/'));
    let (body, response) = crate::http::get_text(&latest, Some("application/json"))?;
    if response.status != 200 {
        return Err(format!("{latest} answered {}", response.status));
    }
    let tag = latest_tag(&body, &response.final_url).ok_or("no release tag in the answer")?;
    let yml_url = download_url(&feed.url, &tag, LATEST_YML);
    let (text, response) = crate::http::get_text(&yml_url, None)?;
    if response.status != 200 {
        return Err(format!("{yml_url} answered {}", response.status));
    }
    let info = parse_latest_yml(&text).ok_or_else(|| format!("{LATEST_YML} could not be read"))?;
    if !is_newer(&info.version, &feed.current) {
        log.info(
            "update check: no newer version",
            Some(json!({ "latest": info.version, "running": feed.current })),
        );
        return Ok(Found::Nothing);
    }
    match mode {
        UpdateMode::None => Ok(Found::Nothing),
        UpdateMode::Notify => Ok(Found::Available(info.version)),
        UpdateMode::Install => {
            if have == Some(info.version.as_str()) {
                return Ok(Found::Nothing);
            }
            let file = info
                .installer()
                .ok_or_else(|| format!("{LATEST_YML} names no installer"))?;
            let url = download_url(&feed.url, &tag, &file.url);
            log.info(
                "update found, downloading",
                Some(json!({ "version": info.version, "url": url })),
            );
            let installer = download(&url, &feed.cache, &file.sha512, file.size)?;
            Ok(Found::Ready(Ready {
                version: info.version,
                installer,
            }))
        }
    }
}

/// Writes through to a file and a hash at once.
struct Hashing<'a> {
    file: &'a mut std::fs::File,
    hash: Sha512,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.file.write_all(bytes)?;
        self.hash.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Downloads the installer into `cache\pending` and checks it; a file that does not match is
/// thrown away.
fn download(url: &str, cache: &Path, sha512: &str, size: Option<u64>) -> Result<PathBuf, String> {
    let pending = cache.join("pending");
    // Only the newest download is kept.
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(&pending).map_err(|error| format!("{}: {error}", pending.display()))?;
    let target = pending.join(file_name_of(url));
    let part = pending.join("download.part");
    let mut file = std::fs::File::create(&part).map_err(|error| format!("{}: {error}", part.display()))?;
    let mut sink = Hashing {
        file: &mut file,
        hash: Sha512::new(),
    };
    let result = crate::http::get(url, None, &mut sink, MAX_INSTALLER);
    let digest = base64::engine::general_purpose::STANDARD.encode(sink.hash.finalize());
    drop(file);
    let checked = result.and_then(|response| {
        if response.status != 200 {
            return Err(format!("{url} answered {}", response.status));
        }
        if size.is_some_and(|size| size != response.length) {
            return Err(format!("the installer has {} bytes, not {size:?}", response.length));
        }
        if digest != sha512 {
            return Err(format!("the installer's SHA-512 does not match {LATEST_YML}"));
        }
        Ok(())
    });
    if let Err(error) = checked {
        let _ = std::fs::remove_file(&part);
        return Err(error);
    }
    std::fs::rename(&part, &target).map_err(|error| format!("{}: {error}", target.display()))?;
    Ok(target)
}

/// Clears what earlier updates left (also what the Electron version's updater kept there): at a
/// start nothing waits to be installed any more.
pub fn clear_cache(cache: &Path) {
    let _ = std::fs::remove_dir_all(cache);
}

/// Starts the installer, which waits for reframe to be gone. `restart`: it starts reframe again
/// afterwards and shows its progress ("Restart to update"); else it installs without a window
/// (on quit). `fake`: the end-to-end tests' stand-in (`REFRAME_FAKE_INSTALLER`, a script run by
/// node), which gets the installer and its arguments.
#[cfg(windows)]
pub fn run_installer(installer: &Path, restart: bool, fake: Option<(&str, &str)>) -> std::io::Result<()> {
    let args = installer_args(!restart, restart);
    let command = || {
        let mut command = match fake {
            Some((node, script)) => {
                let mut command = Command::new(node);
                command.arg(script).arg(installer);
                command
            }
            None => Command::new(installer),
        };
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    };
    // It has to outlive reframe, also when something started reframe in a job that ends with it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command()
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB)
            .spawn()
            .or_else(|_| {
                command()
                    .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
                    .spawn()
            })
            .map(|_| ())
    }
    // In a session of its own, so that nothing that ends reframe's ends it too.
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        command().process_group(0).spawn().map(|_| ())
    }
}

/// Puts a downloaded AppImage in the place of the running one (`APPIMAGE`), as electron-updater
/// did for 0.x (see [`reframe_core::updates::app_image_destination`]), and says where it went.
/// `restart`: starts it ("Restart to update"); it waits for this reframe to be gone. Linux.
#[cfg(not(windows))]
pub fn install_app_image(downloaded: &Path, running: &Path, restart: bool) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    let name = downloaded
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let destination = reframe_core::updates::app_image_destination(running, &name);
    // The running one stays readable to this process (it is mounted) until it ends.
    std::fs::remove_file(running)?;
    if std::fs::rename(downloaded, &destination).is_err() {
        // The cache is on another file system.
        std::fs::copy(downloaded, &destination)?;
        let _ = std::fs::remove_file(downloaded);
    }
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o755))?;
    if restart {
        Command::new(&destination)
            .env("APPIMAGE_SILENT_INSTALL", "true")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
    }
    Ok(destination)
}
