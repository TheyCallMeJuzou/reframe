//! Translated from the Electron version's `src/main/claude/discovery.test.ts`, one test per case.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::*;

fn line(value: Value) -> String {
    format!("{value}\n")
}

fn user(content: &str) -> String {
    user_in(content, "C:\\work\\api")
}

fn user_in(content: &str, cwd: &str) -> String {
    line(json!({ "type": "user", "cwd": cwd, "message": { "content": content } }))
}

fn set_age(file: &Path, age_ms: u64) {
    let time = SystemTime::now() - Duration::from_millis(age_ms);
    fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn transcript(home: &Path, project: &str, id: &str, body: &str, age_ms: u64) -> PathBuf {
    let dir = home.join("projects").join(project);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{id}.jsonl"));
    fs::write(&file, body).unwrap();
    set_age(&file, age_ms);
    file
}

fn summary_of(cwd: &str, title: &str) -> Option<TranscriptSummary> {
    Some(TranscriptSummary {
        cwd: Some(cwd.to_owned()),
        title: Some(title.to_owned()),
    })
}

fn set(values: &[&str]) -> HashSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn ids(index: &mut TranscriptIndex, query: Option<&str>, exclude: &[&str]) -> Vec<String> {
    let exclude = set(exclude);
    let page = index.list(&ListOptions {
        query,
        limit: 10,
        exclude: &exclude,
    });
    page.items.into_iter().map(|item| item.session_id).collect()
}

// ---- readTranscriptInfo ----

#[test]
fn prefers_the_agent_name_then_the_latest_ai_title_then_prompts() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    let titled = transcript(
        h,
        "p",
        "a",
        &(user("fix the login")
            + &line(json!({ "type": "ai-title", "aiTitle": "Login fix" }))
            + &line(json!({ "type": "ai-title", "aiTitle": "Login and signup fix" }))),
        0,
    );
    assert_eq!(
        read_transcript_info(&titled).unwrap(),
        summary_of("C:\\work\\api", "Login and signup fix")
    );
    let named = transcript(
        h,
        "p",
        "b",
        &(user("x") + &line(json!({ "type": "agent-name", "agentName": "api-work" }))),
        0,
    );
    assert_eq!(
        read_transcript_info(&named).unwrap().unwrap().title.as_deref(),
        Some("api-work")
    );
    let plain = transcript(
        h,
        "p",
        "c",
        &(user("<command-name>/clear</command-name>") + &user("   explain   this  ")),
        0,
    );
    assert_eq!(
        read_transcript_info(&plain).unwrap().unwrap().title.as_deref(),
        Some("explain this")
    );
}

#[test]
fn skips_transcripts_without_a_prompt_and_survives_broken_lines() {
    let home = tempfile::tempdir().unwrap();
    let file = transcript(
        home.path(),
        "p",
        "a",
        &(line(json!({ "type": "mode" })) + "{broken\n"),
        0,
    );
    assert_eq!(read_transcript_info(&file).unwrap(), None);
}

#[test]
fn reads_the_title_from_the_end_of_a_large_transcript() {
    let home = tempfile::tempdir().unwrap();
    let filler = line(json!({ "type": "assistant", "text": "x".repeat(1000) })).repeat(600);
    let body = user("start") + &filler + &line(json!({ "type": "ai-title", "aiTitle": "Late title" }));
    let file = transcript(home.path(), "p", "a", &body, 0);
    assert!(fs::metadata(&file).unwrap().len() > 400 * 1024);
    assert_eq!(
        read_transcript_info(&file).unwrap(),
        summary_of("C:\\work\\api", "Late title")
    );
}

// ---- TranscriptIndex ----

#[test]
fn lists_newest_first_excludes_known_sessions_searches_title_and_folder_and_pages() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    transcript(h, "p1", "old", &user_in("old work", "C:\\work\\web"), 60_000);
    transcript(h, "p1", "mid", &user_in("middle", "C:\\work\\api"), 30_000);
    transcript(h, "p2", "new", &user_in("newest", "C:\\work\\api"), 0);
    transcript(h, "p2", "mine", &user("managed by reframe"), 0);
    let mut index = TranscriptIndex::new(h);

    assert_eq!(ids(&mut index, None, &["mine"]), ["new", "mid", "old"]);
    assert_eq!(ids(&mut index, Some("WEB"), &["mine"]), ["old"]);
    assert_eq!(ids(&mut index, Some("middle"), &["mine"]), ["mid"]);
    let page = index.list(&ListOptions {
        query: None,
        limit: 2,
        exclude: &HashSet::new(),
    });
    assert!(page.has_more);
}

#[test]
fn lists_a_project_folder_again_only_once_it_was_named_as_changed() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    transcript(h, "p1", "a", &user("first"), 60_000);
    transcript(h, "p2", "b", &user("second"), 30_000);
    let mut index = TranscriptIndex::new(h);
    assert_eq!(ids(&mut index, None, &[]), ["b", "a"]);

    // Used again a moment ago: its time changed, but nobody said so yet.
    set_age(&h.join("projects").join("p1").join("a.jsonl"), 10_000);
    transcript(h, "p2", "c", &user("third"), 0);
    assert_eq!(ids(&mut index, None, &[]), ["b", "a"]);
    index.invalidate(["p1"]);
    assert_eq!(ids(&mut index, None, &[]), ["a", "b"]);
    index.invalidate_all();
    assert_eq!(ids(&mut index, None, &[]), ["c", "a", "b"]);
    // A folder that is gone takes its transcripts along.
    fs::remove_dir_all(h.join("projects").join("p2")).unwrap();
    index.invalidate(["p2"]);
    assert_eq!(ids(&mut index, None, &[]), ["a"]);
}

// ---- listRunning ----

fn write_entry(home: &Path, pid: u32, session_id: &str, cwd: &str) {
    let entry = json!({ "pid": pid, "sessionId": session_id, "cwd": cwd });
    fs::write(home.join("sessions").join(format!("{pid}.json")), entry.to_string()).unwrap();
}

fn session_ids(running: Vec<RegistryEntry>) -> Vec<String> {
    running.into_iter().map(|entry| entry.session_id).collect()
}

/// The Electron version's case asks the system whether each process runs (its own does, 999999999 does not);
/// here that is passed in, so the test says what the system would.
#[test]
fn lists_live_registry_entries_that_reframe_does_not_own() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    fs::create_dir(h.join("sessions")).unwrap();
    let own_pid = std::process::id();
    write_entry(h, own_pid, "outside", "C:\\x");
    write_entry(h, 999_999_999, "dead", "C:\\x");
    let alive = |entry: &RegistryEntry| entry.pid == own_pid;
    let exclude = |pids: &[u32], session_ids: &[&str]| RunningExclusion {
        pids: pids.iter().copied().collect(),
        session_ids: set(session_ids),
    };
    assert_eq!(session_ids(list_running(h, &exclude(&[], &[]), &alive)), ["outside"]);
    assert_eq!(list_running(h, &exclude(&[own_pid], &[]), &alive), []);
    assert_eq!(list_running(h, &exclude(&[], &["outside"]), &alive), []);
}

/// There is one `list_running` here, which blocks; this is the case of the Electron version's `listRunningAsync`.
#[test]
fn lists_them_without_blocking_too_asking_whether_each_runs_in_wsl_through_its_proc() {
    let home = tempfile::tempdir().unwrap();
    let h = home.path();
    fs::create_dir(h.join("sessions")).unwrap();
    write_entry(h, 10, "running", "/x");
    write_entry(h, 11, "gone", "/x");
    write_entry(h, 12, "own", "/x");
    fs::write(h.join("sessions").join("13.json"), "{ half written").unwrap();
    let alive = |entry: &RegistryEntry| entry.pid != 11;
    let none = RunningExclusion::default();
    let mut found = session_ids(list_running(h, &none, &alive));
    found.sort();
    assert_eq!(found, ["own", "running"]);
    let own = RunningExclusion {
        pids: HashSet::new(),
        session_ids: set(&["own"]),
    };
    assert_eq!(session_ids(list_running(h, &own, &alive)), ["running"]);
    assert_eq!(list_running(&h.join("missing"), &none, &alive), []);
}

// ---- Not in the Electron version's tests: where Rust's strings differ from JavaScript's ----

#[test]
fn cuts_a_long_title_by_utf16_length_without_splitting_a_character() {
    assert_eq!(truncate(&"a".repeat(90), 90), "a".repeat(90));
    assert_eq!(truncate(&"a".repeat(91), 90), "a".repeat(89) + "…");
    // 44 emoji are 88 units: one more fits `slice(0, 89)` only by half, and is dropped.
    assert_eq!(truncate(&"😀".repeat(46), 90), "😀".repeat(44) + "…");
    assert_eq!(truncate(&"😀".repeat(45), 90), "😀".repeat(45));
    assert_eq!(collapse_whitespace("\u{FEFF} a\t\n b\u{A0}"), "a b");
}

#[test]
fn keeps_a_line_that_holds_half_a_surrogate_pair() {
    let entries = parse_lines(
        "{\"type\":\"ai-title\",\"aiTitle\":\"cut \\ud83d\"}\n{\"type\":\"ai-title\",\"aiTitle\":\"\\ud83d\\ude00 \\\\ud83d\"}\n",
    );
    assert_eq!(entries[0]["aiTitle"], "cut \u{FFFD}");
    assert_eq!(entries[1]["aiTitle"], "😀 \\ud83d");
}
