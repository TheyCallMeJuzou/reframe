//! What the updater decides without a network: which version is newer, what a release's
//! `latest.yml` says, where its files are, and how the installer is started.
//!
//! The feed is the one the Electron version's updater (electron-updater, GitHub provider) reads,
//! so that installed 0.x copies and the native app update from the same releases: the newest
//! release's tag, then `latest.yml` among its files, then the installer it names, checked by its
//! SHA-512. Ported from what `src/main/updates.ts` left to electron-updater.

use std::cmp::Ordering;
use std::time::Duration;

/// The releases: `{FEED}/latest` is the newest one, `{FEED}/download/{tag}/{file}` its files.
pub const FEED: &str = "https://github.com/TheyCallMeJuzou/reframe/releases";
/// The first check comes a while after the start, so it does not slow the start down.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
/// Then one every few hours.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Where downloads wait to be installed, under the local app data folder: the folder the
/// Electron version's updater used (`updaterCacheDirName`), so the hand-over leaves one folder.
pub const CACHE_FOLDER: &str = "reframe-updater";

/// A version as releases are tagged: `1.2.3`, maybe with a `v` in front and a pre-release part
/// (`1.0.0-beta.2`), which comes before the release itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        // Build metadata does not count.
        let text = text.split('+').next()?;
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, pre.split('.').map(str::to_owned).collect()),
            None => (text, Vec::new()),
        };
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let version = Version {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
            pre,
        };
        if parts.next().is_some() || version.pre.iter().any(String::is_empty) {
            return None;
        }
        Some(version)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A pre-release comes before its release.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pre-release parts as semver orders them: numbers by value and before words, words as text,
/// and a shorter list first when the rest is equal.
fn compare_pre(a: &[String], b: &[String]) -> Ordering {
    for (a, b) in a.iter().zip(b) {
        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

/// Whether `offered` is newer than `current`. A version that cannot be read is not.
pub fn is_newer(offered: &str, current: &str) -> bool {
    match (Version::parse(offered), Version::parse(current)) {
        (Some(offered), Some(current)) => offered > current,
        _ => false,
    }
}

/// The installer of a release, as `latest.yml` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateFile {
    /// A file name among the release's files, or a full address.
    pub url: String,
    /// Base64 of the file's SHA-512.
    pub sha512: String,
    pub size: Option<u64>,
}

/// What a release's `latest.yml` says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub files: Vec<UpdateFile>,
    pub release_date: Option<String>,
}

impl UpdateInfo {
    /// The file to download: the first one, as electron-updater takes it.
    pub fn installer(&self) -> Option<&UpdateFile> {
        self.files.first()
    }
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 {
        if let Some(inner) = value.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')) {
            return inner.replace("''", "'");
        }
        if let Some(inner) = value.strip_prefix('"').and_then(|value| value.strip_suffix('"')) {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
    }
    value.to_owned()
}

/// Reads `latest.yml` as electron-builder and reframe's release script write it: scalars at the
/// top, `files` as a list of mappings. The older top-level `path` and `sha512` stand in for
/// `files` when it is missing.
pub fn parse_latest_yml(text: &str) -> Option<UpdateInfo> {
    let mut version = None;
    let mut release_date = None;
    let mut path = None;
    let mut sha512 = None;
    let mut files: Vec<UpdateFile> = Vec::new();
    let mut in_files = false;
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indented = trimmed.starts_with([' ', '\t']);
        if !indented {
            in_files = false;
            let (key, value) = trimmed.split_once(':')?;
            match key.trim() {
                "version" => version = Some(unquote(value)),
                "releaseDate" => release_date = Some(unquote(value)),
                "path" => path = Some(unquote(value)),
                "sha512" => sha512 = Some(unquote(value)),
                "files" => in_files = true,
                _ => {}
            }
            continue;
        }
        if !in_files {
            continue;
        }
        let mut entry = trimmed.trim_start();
        if let Some(rest) = entry.strip_prefix('-') {
            files.push(UpdateFile {
                url: String::new(),
                sha512: String::new(),
                size: None,
            });
            entry = rest.trim_start();
            if entry.is_empty() {
                continue;
            }
        }
        let file = files.last_mut()?;
        let (key, value) = entry.split_once(':')?;
        match key.trim() {
            "url" => file.url = unquote(value),
            "sha512" => file.sha512 = unquote(value),
            "size" => file.size = unquote(value).parse().ok(),
            _ => {}
        }
    }
    files.retain(|file| !file.url.is_empty() && !file.sha512.is_empty());
    if files.is_empty()
        && let (Some(url), Some(sha512)) = (path, sha512)
    {
        files.push(UpdateFile {
            url,
            sha512,
            size: None,
        });
    }
    Some(UpdateInfo {
        version: version.filter(|version| !version.is_empty())?,
        files,
        release_date,
    })
}

/// Writes `latest.yml` for a release with one installer, as electron-builder does (0.x copies
/// read it with their updater).
pub fn latest_yml(version: &str, file_name: &str, sha512: &str, size: u64, release_date: &str) -> String {
    format!(
        "version: {version}\nfiles:\n  - url: {file_name}\n    sha512: {sha512}\n    size: {size}\npath: {file_name}\nsha512: {sha512}\nreleaseDate: '{release_date}'\n"
    )
}

/// The newest release's tag, from what `{FEED}/latest` answered: GitHub's JSON for it, or else
/// the address it led to (`…/releases/tag/v1.0.0`).
pub fn latest_tag(body: &str, final_url: &str) -> Option<String> {
    let from_json = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json["tag_name"].as_str().map(str::to_owned));
    from_json.filter(|tag| !tag.is_empty()).or_else(|| {
        let (_, tag) = final_url.split_once("/tag/")?;
        let tag = tag.split(['?', '#']).next()?.trim_end_matches('/');
        (!tag.is_empty() && !tag.contains('/')).then(|| tag.to_owned())
    })
}

/// Where a file of a release is: a full address as it is, a file name among the release's
/// files (with spaces as dashes, as GitHub stores them).
pub fn download_url(feed: &str, tag: &str, file: &str) -> String {
    if file.starts_with("https://") || file.starts_with("http://") {
        return file.to_owned();
    }
    format!(
        "{}/download/{tag}/{}",
        feed.trim_end_matches('/'),
        file.replace(' ', "-")
    )
}

/// The name a download is kept under: the file name of its address, without anything that
/// could lead out of the folder.
pub fn file_name_of(url: &str) -> String {
    let name = url
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    if name.is_empty() || name.starts_with('.') {
        "reframe-Setup.exe".to_owned()
    } else {
        name
    }
}

/// The installer's arguments, as electron-updater passes them: `--updated` always, `/S` to
/// install without a window (on quit), `--force-run` to start reframe again afterwards
/// ("Restart to update").
pub fn installer_args(silent: bool, force_run: bool) -> Vec<&'static str> {
    let mut args = vec!["--updated"];
    if silent {
        args.push("/S");
    }
    if force_run {
        args.push("--force-run");
    }
    args
}

/// An address as WinHTTP takes it apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// With the query.
    pub path: String,
}

pub fn parse_url(url: &str) -> Option<Url> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (authority, path) = match rest.find(['/', '?']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let path = if path.starts_with('?') {
        format!("/{path}")
    } else {
        path.to_owned()
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => (host, port.parse().ok()?),
        _ => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() || host.contains('@') {
        return None;
    }
    Some(Url {
        secure,
        host: host.to_owned(),
        port,
        path,
    })
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
