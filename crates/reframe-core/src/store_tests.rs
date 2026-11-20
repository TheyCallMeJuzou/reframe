use super::*;

use tempfile::TempDir;

/// A store in a folder of its own, which goes when the test ends.
fn store() -> (TempDir, StateStore) {
    let dir = tempfile::Builder::new().prefix("reframe-store-").tempdir().unwrap();
    let store = StateStore::new(dir.path().join("state.json"));
    (dir, store)
}

fn with_folder(folder: &str) -> AppState {
    AppState {
        recent_folders: vec![folder.to_owned()],
        ..AppState::default()
    }
}

fn names(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn backup_folders(store: &StateStore) -> Vec<String> {
    let text = fs::read_to_string(store.backup_file()).unwrap();
    AppState::parse(&text).unwrap().recent_folders
}

#[test]
fn gives_files_from_before_preferences_existed_the_default_preferences() {
    let (_dir, mut s) = store();
    let mut old = serde_json::to_value(AppState::default()).unwrap();
    old.as_object_mut().unwrap().remove("preferences").unwrap();
    fs::write(s.file(), serde_json::to_string(&old).unwrap()).unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::File);
    assert!(loaded.state.preferences.name_from_folder);
}

#[test]
fn will_not_start_on_a_state_file_that_is_there_but_cannot_be_read() {
    // (As the Electron version: its start failed, rather than running on without saving.)
    let (dir, mut store) = store();
    fs::create_dir(store.file()).unwrap();
    assert!(store.load().is_err());
    assert_eq!(names(&dir), ["state.json"]);
}

#[test]
fn starts_from_defaults_without_a_file() {
    let (_dir, mut s) = store();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::Default);
    assert_eq!(loaded.state.windows.len(), 1);
    assert_eq!(loaded.rejected, None);
    assert!(!s.is_read_only());
}

#[test]
fn round_trips_state_and_keeps_the_previous_version_as_backup() {
    let (dir, mut s) = store();
    s.save_now(&with_folder("one")).unwrap();
    s.save_now(&with_folder("two")).unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::File);
    assert_eq!(loaded.state.recent_folders, ["two"]);
    assert_eq!(backup_folders(&s), ["one"]);
    assert_eq!(names(&dir), ["state.json", "state.json.bak"]);
    // The exact text the Electron version writes.
    assert_eq!(fs::read_to_string(s.file()).unwrap(), loaded.state.to_json());
}

#[test]
fn falls_back_to_the_backup_when_the_file_is_corrupt_or_invalid() {
    let (_dir, mut s) = store();
    s.save_now(&with_folder("one")).unwrap();
    s.save_now(&with_folder("two")).unwrap();
    fs::write(s.file(), r#"{ "version": 1, "sessions": "#).unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::Backup);
    assert_eq!(loaded.state.recent_folders, ["one"]);
    fs::write(s.file(), r#"{"version":1,"sessions":"nope"}"#).unwrap();
    assert_eq!(s.load().unwrap().source, LoadSource::Backup);
}

#[test]
fn sets_an_unusable_file_aside_instead_of_writing_over_it() {
    let (_dir, mut s) = store();
    s.save_now(&with_folder("one")).unwrap();
    s.save_now(&with_folder("two")).unwrap();
    let broken = r#"{"version":1,"sessions":"nope"}"#;
    fs::write(s.file(), broken).unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::Backup);
    assert_eq!(loaded.state.recent_folders, ["one"]);
    let rejected = loaded.rejected.unwrap();
    assert!(rejected.reason.contains("sessions"), "{}", rejected.reason);
    // ^state\.json\.rejected-\d{8}-\d{6}$
    let name = rejected.file.file_name().unwrap().to_str().unwrap();
    let stamp = name.strip_prefix("state.json.rejected-").unwrap();
    assert_eq!(stamp.len(), 15, "{name}");
    assert!(
        stamp
            .char_indices()
            .all(|(at, c)| if at == 8 { c == '-' } else { c.is_ascii_digit() }),
        "{name}"
    );
    assert_eq!(rejected.file.parent(), s.file().parent());
    // Saved twice, as a start and a window move do: the set-aside file and the backup's content survive.
    s.save_now(&loaded.state).unwrap();
    s.save_now(&with_folder("three")).unwrap();
    assert_eq!(fs::read_to_string(&rejected.file).unwrap(), broken);
    assert_eq!(backup_folders(&s), ["one"]);
}

#[test]
fn sets_aside_an_unusable_file_without_a_backup_too_and_keeps_only_the_last_five() {
    let (dir, mut s) = store();
    for i in 0..7 {
        fs::write(s.file(), format!("garbage {i}")).unwrap();
        let loaded = s.load().unwrap();
        assert_eq!(loaded.source, LoadSource::Default);
        assert!(loaded.rejected.unwrap().reason.contains("not JSON"));
    }
    let kept: Vec<String> = names(&dir).into_iter().filter(|f| f.contains(".rejected-")).collect();
    assert_eq!(kept.len(), 5);
    let mut contents: Vec<String> = kept
        .iter()
        .map(|f| fs::read_to_string(dir.path().join(f)).unwrap())
        .collect();
    contents.sort();
    assert_eq!(
        contents,
        ["garbage 2", "garbage 3", "garbage 4", "garbage 5", "garbage 6"]
    );
}

#[test]
fn leaves_a_backup_from_a_newer_reframe_alone_as_well() {
    let (_dir, mut s) = store();
    let newer = r#"{"version":2,"sessions":{},"somethingNew":true}"#;
    fs::write(s.backup_file(), newer).unwrap();
    fs::write(s.file(), "garbage").unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::Newer);
    assert!(loaded.rejected.unwrap().reason.contains("not JSON"));
    assert!(s.is_read_only());
    s.save_now(&with_folder("one")).unwrap();
    assert!(!s.file().exists());
    assert_eq!(fs::read_to_string(s.backup_file()).unwrap(), newer);
}

#[test]
fn leaves_a_file_from_a_newer_reframe_alone_and_writes_nothing() {
    let (dir, mut s) = store();
    let newer = r#"{"version":2,"sessions":{},"somethingNew":true}"#;
    fs::write(s.file(), newer).unwrap();
    let loaded = s.load().unwrap();
    assert_eq!(loaded.source, LoadSource::Newer);
    assert_eq!(loaded.rejected, None);
    assert!(s.is_read_only());
    s.save_now(&with_folder("one")).unwrap();
    s.mark_dirty();
    s.flush(&with_folder("two")).unwrap();
    assert_eq!(fs::read_to_string(s.file()).unwrap(), newer);
    assert_eq!(names(&dir), ["state.json"]);
}

#[test]
fn does_not_overwrite_a_good_backup_with_a_broken_file() {
    let (_dir, mut s) = store();
    s.save_now(&with_folder("one")).unwrap();
    s.save_now(&with_folder("two")).unwrap();
    fs::write(s.file(), "garbage").unwrap();
    s.save_now(&with_folder("three")).unwrap();
    assert_eq!(backup_folders(&s), ["one"]);
}

#[test]
fn coalesces_debounced_saves_and_flushes_on_demand() {
    let (_dir, mut s) = store();
    // Nothing waiting: nothing is written.
    s.flush(&with_folder("none")).unwrap();
    assert!(!s.file().exists());
    // Two changes, the state being "a" and then "b": one wait, started by the first.
    let before = Instant::now();
    assert!(s.mark_dirty());
    let due = s.save_due_at().unwrap();
    assert!(!s.mark_dirty());
    assert_eq!(s.save_due_at(), Some(due));
    assert!(due >= before + SAVE_DELAY && due <= Instant::now() + SAVE_DELAY);
    assert!(s.is_dirty());
    assert!(!s.file().exists());
    assert!(!s.is_due(before));
    // 600 ms later the caller's timer has fired.
    assert!(s.is_due(before + Duration::from_millis(600)));
    s.flush(&with_folder("b")).unwrap();
    assert!(!s.is_dirty());
    assert_eq!(s.save_due_at(), None);
    assert_eq!(s.load().unwrap().state.recent_folders, ["b"]);
    assert!(s.mark_dirty());
    s.flush(&with_folder("c")).unwrap();
    assert_eq!(s.load().unwrap().state.recent_folders, ["c"]);
    // A save in between cancels the waiting one.
    assert!(s.mark_dirty());
    s.save_now(&with_folder("d")).unwrap();
    assert!(!s.is_dirty());
}

// Not in store.test.ts: the parts that Node did there.

#[test]
fn stamps_set_aside_files_with_the_utc_date_and_time() {
    let at = |seconds: u64| utc_stamp(UNIX_EPOCH + Duration::from_secs(seconds));
    assert_eq!(at(0), "19700101-000000");
    assert_eq!(at(951_782_399), "20000228-235959");
    assert_eq!(at(951_782_400), "20000229-000000");
    assert_eq!(at(951_868_800), "20000301-000000");
    assert_eq!(at(1_794_400_496), "20261111-123456");
    assert_eq!(at(4_107_542_399), "21000228-235959");
    assert_eq!(at(4_107_542_400), "21000301-000000");
    assert_eq!(utc_stamp(UNIX_EPOCH - Duration::from_secs(1)), "19700101-000000");
}

#[test]
fn numbers_files_set_aside_within_the_same_second() {
    assert_eq!(number_of("state.json.rejected-20261111-123456"), None);
    assert_eq!(number_of("state.json.rejected-20261111-123456-002"), Some(2));
    assert_eq!(number_of("state.json.rejected-20261111-123456-1000"), None);
    assert_eq!(number_of("-07"), None);
}

#[test]
fn says_which_key_of_an_invalid_file_is_wrong() {
    let (_dir, mut s) = store();
    let reason = |s: &mut StateStore, text: &str| {
        fs::write(s.file(), text).unwrap();
        s.load().unwrap().rejected.unwrap().reason
    };
    let both = reason(
        &mut s,
        r#"{"version":1,"sessions":"nope","windows":3,"recentFolders":[]}"#,
    );
    assert!(both.starts_with("sessions: ") && both.contains("; windows: "), "{both}");
    assert!(reason(&mut s, "[]").starts_with("(file): "));
    // A version that is not whole is a newer one too, if it is above this one.
    fs::write(s.file(), r#"{"version":1.5}"#).unwrap();
    assert_eq!(s.load().unwrap().source, LoadSource::Newer);
}

#[test]
fn retries_a_rename_only_on_sharing_errors() {
    assert!(is_sharing_error(&io::Error::from(io::ErrorKind::PermissionDenied)));
    assert!(!is_sharing_error(&io::Error::from(io::ErrorKind::NotFound)));
    #[cfg(windows)]
    for code in [5, 32, 33] {
        assert!(is_sharing_error(&io::Error::from_raw_os_error(code)));
    }
    let dir = tempfile::tempdir().unwrap();
    let missing = rename_with_retry(&dir.path().join("none"), &dir.path().join("to"));
    assert_eq!(missing.unwrap_err().kind(), io::ErrorKind::NotFound);
}
