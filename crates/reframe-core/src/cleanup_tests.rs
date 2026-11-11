use super::*;

use std::fs::File;
use std::time::Duration;

fn names(dir: &Path) -> Vec<String> {
    let mut names = names_in(dir);
    names.sort();
    names
}

fn set_of(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// Opens a folder so that its times can be set.
fn open_folder(dir: &Path) -> File {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_BACKUP_SEMANTICS: without it Windows opens no folders.
        fs::OpenOptions::new()
            .write(true)
            .custom_flags(0x0200_0000)
            .open(dir)
            .unwrap()
    }
    #[cfg(not(windows))]
    {
        File::open(dir).unwrap()
    }
}

#[test]
fn picks_entries_older_than_the_limit_and_hook_files_of_sessions_that_are_gone() {
    let entry = |name: &str, mtime_ms: f64| Entry {
        name: name.to_owned(),
        mtime_ms,
    };
    assert_eq!(
        older_than(&[entry("old", 0.0), entry("new", 900.0)], 1000.0, 500.0),
        ["old"]
    );
    let files = ["a.json", "b.json", "notes.txt"].map(str::to_owned);
    assert_eq!(stray_hook_files(&files, &set_of(&["a"])), ["b.json"]);
}

#[test]
fn removes_old_drops_and_stray_hook_files_and_nothing_else() {
    let root = tempfile::Builder::new().prefix("reframe-cleanup-").tempdir().unwrap();
    let drops = root.path().join("drops");
    let hooks = root.path().join("hooks");
    for name in ["old", "new"] {
        fs::create_dir_all(drops.join(name)).unwrap();
        fs::write(drops.join(name).join("image.png"), "").unwrap();
    }
    let long_ago = SystemTime::now() - Duration::from_secs_f64(DROPS_KEEP_MS / 1_000.0) - Duration::from_secs(60);
    open_folder(&drops.join("old")).set_modified(long_ago).unwrap();
    fs::create_dir(&hooks).unwrap();
    fs::write(hooks.join("kept.json"), "{}").unwrap();
    fs::write(hooks.join("gone.json"), "{}").unwrap();

    assert_eq!(
        clean_up(&drops, &hooks, &set_of(&["kept"])),
        Removed { drops: 1, hooks: 1 }
    );
    assert_eq!(names(&drops), ["new"]);
    assert_eq!(names(&hooks), ["kept.json"]);
    // Folders that are not there are nothing to clean.
    let none = root.path().join("none");
    assert_eq!(clean_up(&none, &none, &HashSet::new()), Removed { drops: 0, hooks: 0 });
}
