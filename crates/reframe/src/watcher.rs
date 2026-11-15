//! Watches a Claude Code folder for what the session list shows: its session registry (claudes
//! starting and ending) and its transcripts (sessions being used). Changes come in batches, with
//! the project folders they touched, so the transcripts are listed again only there. A folder
//! that is not there yet is looked for every couple of seconds; one that cannot be watched counts
//! as changed every little while instead.
//!
//! Ported from the Electron version's `src/main/claude/watcher.ts`. It runs on a thread of its
//! own, which ends once nobody listens any more.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use notify::{RecursiveMode, Watcher as _};

/// What changed in a batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Something in the session registry changed.
    pub registry: bool,
    /// Project folders whose transcripts changed; `None` when that is not known (all of them).
    pub projects: Option<HashSet<String>>,
}

#[derive(Clone, Copy)]
pub struct Timing {
    /// How long changes are collected before they are told.
    pub batch: Duration,
    /// How often a missing (or not yet watched) folder is looked for.
    pub look: Duration,
    /// How often a folder that cannot be watched counts as changed.
    pub fallback: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            batch: Duration::from_millis(1000),
            look: Duration::from_millis(2000),
            fallback: Duration::from_millis(30_000),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Folder {
    Sessions,
    Projects,
}

const FOLDERS: [(Folder, &str, RecursiveMode); 2] = [
    (Folder::Sessions, "sessions", RecursiveMode::NonRecursive),
    (Folder::Projects, "projects", RecursiveMode::Recursive),
];

/// What the watchers report to the watching thread.
enum Seen {
    /// A file changed (its path), or something without a path.
    Change(Folder, Option<PathBuf>),
    /// A watcher broke (its folder was removed, say).
    Broken(Folder),
}

/// Starts watching `home`; batches of changes come out of the receiver. Dropping it ends the
/// watching.
pub fn watch(home: PathBuf, timing: Timing) -> UnboundedReceiver<Changes> {
    let (sender, receiver) = unbounded();
    std::thread::Builder::new()
        .name("claude-watcher".into())
        .spawn(move || run(&home, timing, &sender))
        .expect("could not start the watcher thread");
    receiver
}

fn run(home: &Path, timing: Timing, out: &UnboundedSender<Changes>) {
    let (seen_tx, seen) = channel::<Seen>();
    let mut watchers: HashMap<Folder, notify::RecommendedWatcher> = HashMap::new();
    let mut unwatchable: HashMap<Folder, Instant> = HashMap::new();
    let mut pending: Option<Changes> = None;
    let mut batch_until: Option<Instant> = None;
    let mut next_look = Instant::now();

    let note = |folder: Folder, file: Option<&Path>, pending: &mut Option<Changes>, until: &mut Option<Instant>| {
        let name = |path: &Path| path.file_name().map(|name| name.to_string_lossy().into_owned());
        // Claude Code keeps other files next to the registry's (<pid>.json) there.
        if folder == Folder::Sessions
            && let Some(file) = file
            && !name(file).is_some_and(|name| name.ends_with(".json"))
            && file != home.join("sessions")
        {
            return;
        }
        let changes = pending.get_or_insert_with(|| Changes {
            registry: false,
            projects: Some(HashSet::new()),
        });
        match folder {
            Folder::Sessions => changes.registry = true,
            Folder::Projects => {
                // The project folder is the first part below `projects`.
                let project = file
                    .and_then(|file| file.strip_prefix(home.join("projects")).ok())
                    .and_then(|rest| {
                        rest.components()
                            .next()
                            .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    });
                match (project, &mut changes.projects) {
                    (Some(project), Some(projects)) => {
                        projects.insert(project);
                    }
                    _ => changes.projects = None,
                }
            }
        }
        // Told a moment after the first change, however many follow, so a busy session does not
        // hold it back.
        until.get_or_insert_with(|| Instant::now() + timing.batch);
    };

    loop {
        let now = Instant::now();
        if now >= next_look {
            next_look = now + timing.look;
            // Starts watching the folders that are there and not watched yet.
            for (folder, name, mode) in FOLDERS {
                if watchers.contains_key(&folder) {
                    continue;
                }
                let dir = home.join(name);
                if !dir.is_dir() {
                    continue;
                }
                if let Some(failed) = unwatchable.get(&folder) {
                    if failed.elapsed() < timing.fallback {
                        continue;
                    }
                    unwatchable.remove(&folder);
                }
                let tx = seen_tx.clone();
                let made = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    let _ = match event {
                        Ok(event) if event.paths.is_empty() => tx.send(Seen::Change(folder, None)),
                        Ok(event) => event
                            .paths
                            .into_iter()
                            .try_for_each(|path| tx.send(Seen::Change(folder, Some(path)))),
                        Err(_) => tx.send(Seen::Broken(folder)),
                    };
                })
                .and_then(|mut watcher| watcher.watch(&dir, mode).map(|()| watcher));
                match made {
                    Ok(watcher) => {
                        watchers.insert(folder, watcher);
                    }
                    Err(_) => {
                        unwatchable.insert(folder, Instant::now());
                    }
                }
                // What it holds already: it may have just appeared, or it is looked at instead of
                // watched.
                note(folder, None, &mut pending, &mut batch_until);
            }
            // A watched folder that is gone is looked for again like a missing one.
            for (folder, name, _) in FOLDERS {
                if watchers.contains_key(&folder) && !home.join(name).is_dir() {
                    watchers.remove(&folder);
                    note(folder, None, &mut pending, &mut batch_until);
                }
            }
        }
        if let Some(until) = batch_until
            && Instant::now() >= until
        {
            batch_until = None;
            if let Some(changes) = pending.take()
                && out.unbounded_send(changes).is_err()
            {
                return;
            }
        }
        if out.is_closed() {
            return;
        }
        let wake = batch_until.map_or(next_look, |until| until.min(next_look));
        match seen.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(Seen::Change(folder, path)) => note(folder, path.as_deref(), &mut pending, &mut batch_until),
            Ok(Seen::Broken(folder)) => {
                watchers.remove(&folder);
                note(folder, None, &mut pending, &mut batch_until);
            }
            Err(RecvTimeoutError::Timeout) => {}
            // Cannot happen while `seen_tx` is held here.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> Timing {
        Timing {
            batch: Duration::from_millis(150),
            look: Duration::from_millis(100),
            fallback: Duration::from_millis(500),
        }
    }

    /// The next batch, waiting at most a few seconds.
    fn next(receiver: &mut UnboundedReceiver<Changes>) -> Option<Changes> {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            match receiver.try_recv() {
                Ok(changes) => return Some(changes),
                Err(error) if error.is_empty() => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => return None,
            }
        }
        None
    }

    #[test]
    fn starts_watching_a_folder_once_it_appears() {
        let home = tempfile::tempdir().unwrap();
        let mut changes = watch(home.path().to_owned(), quick());
        std::fs::create_dir(home.path().join("sessions")).unwrap();
        // What the folder holds already counts as changed.
        let first = next(&mut changes).unwrap();
        assert!(first.registry);
        // From now on, changes to it are told.
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(home.path().join("sessions").join("123.json"), "{}").unwrap();
        let second = next(&mut changes).unwrap();
        assert!(second.registry);
    }

    #[test]
    fn tells_changes_in_batches_with_the_project_folders_they_touched() {
        let home = tempfile::tempdir().unwrap();
        let projects = home.path().join("projects");
        std::fs::create_dir_all(projects.join("p1")).unwrap();
        std::fs::create_dir_all(projects.join("p2")).unwrap();
        std::fs::create_dir_all(home.path().join("sessions")).unwrap();
        let mut changes = watch(home.path().to_owned(), quick());
        // The first look: everything.
        let first = next(&mut changes).unwrap();
        assert_eq!(first.projects, None);
        std::thread::sleep(Duration::from_millis(300));
        // Neither the registry nor a folder that was not touched.
        std::fs::write(home.path().join("sessions").join("notes.txt"), "x").unwrap();
        std::fs::write(projects.join("p1").join("a.jsonl"), "{}").unwrap();
        std::fs::write(projects.join("p1").join("b.jsonl"), "{}").unwrap();
        let batch = next(&mut changes).unwrap();
        assert!(!batch.registry);
        assert_eq!(batch.projects, Some(HashSet::from(["p1".to_owned()])));
    }
}
