use super::*;

#[test]
fn compares_versions_as_semver_does() {
    assert!(is_newer("1.0.0", "0.10.1"));
    assert!(is_newer("v1.0.1", "1.0.0"));
    assert!(is_newer("0.10.0", "0.9.9"));
    assert!(!is_newer("1.0.0", "1.0.0"));
    assert!(!is_newer("0.10.1", "1.0.0"));
    // A pre-release comes before its release, and after the release before it.
    assert!(is_newer("1.0.0", "1.0.0-beta.2"));
    assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
    assert!(is_newer("1.0.0-beta.2", "0.10.1"));
    assert!(is_newer("1.0.0-beta.10", "1.0.0-beta.2"));
    assert!(is_newer("1.0.0-beta", "1.0.0-alpha.5"));
    assert!(is_newer("1.0.0-beta.1", "1.0.0-beta"));
    // Build metadata does not count; what cannot be read is never newer.
    assert!(!is_newer("1.0.0+abc", "1.0.0"));
    assert!(!is_newer("nonsense", "1.0.0"));
    assert!(!is_newer("1.0", "0.1.0"));
    assert!(!is_newer("2.0.0", "nonsense"));
}

#[test]
fn reads_the_latest_yml_electron_builder_writes() {
    let text = "version: 0.10.0\nfiles:\n  - url: reframe-Setup-0.10.0.exe\n    sha512: 8P6JT3ldEQhD/UDvlF+zZT15twr62wGkwoj4TKjkLa01hk7Xuoarjl4f6wKVmQ==\n    size: 118071643\npath: reframe-Setup-0.10.0.exe\nsha512: 8P6JT3ldEQhD/UDvlF+zZT15twr62wGkwoj4TKjkLa01hk7Xuoarjl4f6wKVmQ==\nreleaseDate: '2026-11-09T12:30:26.846Z'\n";
    let info = parse_latest_yml(text).unwrap();
    assert_eq!(info.version, "0.10.0");
    assert_eq!(info.release_date.as_deref(), Some("2026-11-09T12:30:26.846Z"));
    assert_eq!(
        info.installer(),
        Some(&UpdateFile {
            url: "reframe-Setup-0.10.0.exe".to_owned(),
            sha512: "8P6JT3ldEQhD/UDvlF+zZT15twr62wGkwoj4TKjkLa01hk7Xuoarjl4f6wKVmQ==".to_owned(),
            size: Some(118_071_643),
        })
    );
}

#[test]
fn reads_back_what_the_release_writes() {
    let text = latest_yml(
        "1.0.0",
        "reframe-Setup-1.0.0.exe",
        "abc==",
        42,
        "2026-11-20T10:00:00.000Z",
    );
    let info = parse_latest_yml(&text).unwrap();
    assert_eq!(info.version, "1.0.0");
    assert_eq!(info.files.len(), 1);
    assert_eq!(info.files[0].url, "reframe-Setup-1.0.0.exe");
    assert_eq!(info.files[0].sha512, "abc==");
    assert_eq!(info.files[0].size, Some(42));
    assert_eq!(info.release_date.as_deref(), Some("2026-11-20T10:00:00.000Z"));
}

#[test]
fn takes_the_old_path_and_sha512_when_there_is_no_files_list() {
    let info = parse_latest_yml("version: '1.2.0'\npath: \"reframe Setup.exe\"\nsha512: xyz\n").unwrap();
    assert_eq!(info.version, "1.2.0");
    assert_eq!(info.files[0].url, "reframe Setup.exe");
    assert_eq!(info.files[0].sha512, "xyz");
    // Without a version it is no update info at all; without a file there is nothing to get.
    assert!(parse_latest_yml("files:\n  - url: a.exe\n    sha512: x\n").is_none());
    assert!(parse_latest_yml("<html>Not found</html>").is_none());
    assert!(parse_latest_yml("version: 1.0.0\n").unwrap().installer().is_none());
}

#[test]
fn finds_the_newest_tag_in_the_json_or_the_address_it_led_to() {
    let json = r#"{"id":439106679,"tag_name":"v0.10.0","update_url":"/x/releases/tag/v0.10.0"}"#;
    assert_eq!(latest_tag(json, "").as_deref(), Some("v0.10.0"));
    assert_eq!(
        latest_tag(
            "<html>",
            "https://github.com/TheyCallMeJuzou/reframe/releases/tag/v1.0.0"
        )
        .as_deref(),
        Some("v1.0.0")
    );
    assert_eq!(
        latest_tag("", "https://github.com/o/r/releases/tag/v1.0.0?x=1").as_deref(),
        Some("v1.0.0")
    );
    assert_eq!(latest_tag("<html>", "https://github.com/o/r/releases"), None);
}

#[test]
fn builds_download_addresses_as_electron_updater_does() {
    assert_eq!(
        download_url(FEED, "v1.0.0", "latest.yml"),
        "https://github.com/TheyCallMeJuzou/reframe/releases/download/v1.0.0/latest.yml"
    );
    assert_eq!(
        download_url("http://127.0.0.1:5000/", "v2.0.0", "reframe Setup 2.0.0.exe"),
        "http://127.0.0.1:5000/download/v2.0.0/reframe-Setup-2.0.0.exe"
    );
    assert_eq!(
        download_url(FEED, "v1.0.0", "https://example.com/a.exe"),
        "https://example.com/a.exe"
    );
}

#[test]
fn keeps_downloads_under_a_plain_file_name() {
    assert_eq!(
        file_name_of("https://h/download/v1.0.0/reframe-Setup-1.0.0.exe"),
        "reframe-Setup-1.0.0.exe"
    );
    assert_eq!(file_name_of("https://h/x/..?y"), "reframe-Setup.exe");
    assert_eq!(file_name_of("https://h/a:b*c.exe?z=1"), "abc.exe");
    assert_eq!(file_name_of("https://h/"), "reframe-Setup.exe");
}

#[test]
fn starts_the_installer_as_electron_updater_does() {
    assert_eq!(installer_args(true, false), ["--updated", "/S"]);
    assert_eq!(installer_args(false, true), ["--updated", "--force-run"]);
}

#[test]
fn takes_addresses_apart() {
    assert_eq!(
        parse_url("https://github.com/o/r/releases/latest"),
        Some(Url {
            secure: true,
            host: "github.com".to_owned(),
            port: 443,
            path: "/o/r/releases/latest".to_owned(),
        })
    );
    assert_eq!(
        parse_url("http://127.0.0.1:5123"),
        Some(Url {
            secure: false,
            host: "127.0.0.1".to_owned(),
            port: 5123,
            path: "/".to_owned(),
        })
    );
    assert_eq!(parse_url("http://h?x=1").unwrap().path, "/?x=1");
    assert!(parse_url("ftp://h/x").is_none());
    assert!(parse_url("https://user@h/x").is_none());
    assert!(parse_url("https://h:port/x").is_none());
}
