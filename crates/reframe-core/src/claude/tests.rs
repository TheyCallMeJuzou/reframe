//! Translated from the Electron version's `src/main/claude/claude.test.ts`, one test per case.

use std::collections::HashMap;
use std::path::Path;

use serde_json::json;

use super::args::{LaunchMode, claude_args};
use super::env::claude_env;
use super::registry::*;
use super::resolve::*;
use super::settings::hook_settings;
use crate::state::{LaunchOptions, Lifecycle, PermissionMode, SessionRecord};

fn session() -> SessionRecord {
    SessionRecord {
        id: "app-1".into(),
        claude_session_id: "11111111-2222-4333-8444-555555555555".into(),
        cwd: "C:/work/project".into(),
        name: None,
        claude_name: None,
        launch: LaunchOptions::default(),
        lifecycle: Lifecycle::Open,
        created_at: 0.0,
        last_active_at: 0.0,
        pid: None,
        linux_pid: None,
        used: None,
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

// ---- claudeEnv ----

#[test]
fn drops_only_the_markers_of_a_parent_claude_session() {
    let base = env(&[
        ("PATH", "/bin"),
        ("CLAUDECODE", "1"),
        ("CLAUDE_PID", "42"),
        ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ("CLAUDE_CODE_SESSION_ID", "x"),
        ("CLAUDE_CODE_ENTRYPOINT", "cli"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("CLAUDE_CONFIG_DIR", "/cfg"),
    ]);
    let result = claude_env(base, env(&[("REFRAME_SESSION", "app-1")]));
    assert_eq!(
        result,
        env(&[
            ("PATH", "/bin"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CONFIG_DIR", "/cfg"),
            ("REFRAME_SESSION", "app-1"),
        ])
    );
}

// ---- claudeArgs ----

#[test]
fn starts_a_new_session_with_a_fixed_id_and_options() {
    let mut s = session();
    s.name = Some("api".into());
    s.launch = LaunchOptions {
        model: Some("opus".into()),
        permission_mode: Some(PermissionMode::Plan),
        worktree: Some(true),
        worktree_name: Some("feat".into()),
        worktree_created: None,
    };
    assert_eq!(
        claude_args(&s, LaunchMode::New, Some("S.json")),
        strings(&[
            "--session-id",
            &s.claude_session_id,
            "--name",
            "api",
            "--model",
            "opus",
            "--permission-mode",
            "plan",
            "--worktree",
            "feat",
            "--settings",
            "S.json",
        ])
    );
}

#[test]
fn keeps_the_name_claude_code_was_started_with_when_the_session_is_renamed_in_reframe() {
    let mut s = session();
    s.name = Some("Shown".into());
    s.claude_name = Some(Some("api".into()));
    assert!(claude_args(&s, LaunchMode::Resume, Some("S.json")).contains(&"api".to_owned()));
    s.claude_name = Some(None);
    assert!(!claude_args(&s, LaunchMode::Resume, Some("S.json")).contains(&"--name".to_owned()));
}

#[test]
fn makes_a_worktree_only_on_the_first_start_not_when_started_fresh_in_it_later() {
    let mut s = session();
    s.launch.worktree = Some(true);
    assert!(claude_args(&s, LaunchMode::New, None).contains(&"--worktree".to_owned()));
    s.launch.worktree_created = Some(true);
    assert!(!claude_args(&s, LaunchMode::New, None).contains(&"--worktree".to_owned()));
}

#[test]
fn resumes_without_creating_another_worktree() {
    let mut s = session();
    s.launch.worktree = Some(true);
    assert_eq!(
        claude_args(&s, LaunchMode::Resume, Some("S.json")),
        strings(&["--resume", &s.claude_session_id, "--settings", "S.json"])
    );
}

#[test]
fn passes_no_settings_when_there_are_no_hooks_wsl() {
    let s = session();
    assert_eq!(
        claude_args(&s, LaunchMode::Resume, None),
        strings(&["--resume", &s.claude_session_id])
    );
}

// ---- resolveClaude ----

/// Resolves against a made-up file system: path → text.
fn resolve(
    platform: Platform,
    environment: &[(&str, &str)],
    files: &[(&str, &str)],
    override_path: Option<&str>,
    node_runner: Option<NodeRunner>,
) -> Option<ClaudeBinary> {
    let files: HashMap<String, String> = env(files);
    let environment = env(environment);
    resolve_claude(&ResolveOptions {
        env: &environment,
        platform,
        override_path,
        node_runner,
        home: None,
        is_file: &|path| files.contains_key(path),
        read_file: &|path| files.get(path).cloned(),
    })
}

#[test]
fn follows_path_and_pathext_on_windows_and_unwraps_npm_cmd_shims() {
    let found = resolve(
        Platform::Windows,
        &[("Path", "C:\\npm;C:\\winget"), ("PATHEXT", ".COM;.EXE;.BAT;.CMD")],
        &[
            (
                "C:\\npm\\claude.cmd",
                "@ECHO off\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n",
            ),
            ("C:\\winget\\claude.exe", ""),
        ],
        None,
        None,
    );
    assert_eq!(
        found,
        Some(ClaudeBinary {
            file: "C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe".into(),
            prefix_args: vec![],
            env: HashMap::new(),
            source: "C:\\npm\\claude.cmd".into(),
            via_cmd: None,
        })
    );
}

#[test]
fn runs_the_script_of_a_script_shim_npm_pnpm_with_node_the_one_next_to_it_or_the_one_on_the_path() {
    let npm = [
        "@ECHO off",
        "IF EXIST \"%dp0%\\node.exe\" (",
        "  SET \"_prog=%dp0%\\node.exe\"",
        ") ELSE (",
        "  SET \"_prog=node\"",
        ")",
        "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*",
    ]
    .join("\r\n");
    let pnpm = "@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\..\\claude-code\\cli.mjs\" %*\r\n)";
    let find = |extra: &[(&str, &str)], searched: &str| {
        let mut files = vec![
            ("C:\\npm\\claude.cmd", npm.as_str()),
            ("C:\\Program Files\\nodejs\\node.exe", ""),
        ];
        files.extend_from_slice(extra);
        resolve(
            Platform::Windows,
            &[("Path", searched), ("PATHEXT", ".EXE;.CMD")],
            &files,
            None,
            None,
        )
        .unwrap()
    };

    let found = find(&[], "C:\\npm;C:\\Program Files\\nodejs");
    assert_eq!(found.file, "C:\\Program Files\\nodejs\\node.exe");
    assert_eq!(
        found.prefix_args,
        strings(&["C:\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js"])
    );

    assert_eq!(find(&[("C:\\npm\\node.exe", "")], "C:\\npm").file, "C:\\npm\\node.exe");

    let found = find(&[("C:\\pnpm\\claude.cmd", pnpm)], "C:\\pnpm;C:\\Program Files\\nodejs");
    assert_eq!(found.file, "C:\\Program Files\\nodejs\\node.exe");
    assert_eq!(found.prefix_args, strings(&["C:\\claude-code\\cli.mjs"]));
}

#[test]
fn runs_unknown_cmd_wrappers_through_cmd_exe_with_one_command_line_that_keeps_paths_with_spaces_whole() {
    let found = resolve(
        Platform::Windows,
        &[("PATH", "C:\\My Tools")],
        &[("C:\\My Tools\\claude.cmd", "some other launcher")],
        None,
        None,
    )
    .unwrap();
    assert_eq!(found.file, "cmd.exe");
    assert_eq!(found.via_cmd.as_deref(), Some("C:\\My Tools\\claude.cmd"));
    assert_eq!(
        cmd_command_line(
            found.via_cmd.as_deref().unwrap(),
            &strings(&["--resume", "abc-1", "--name", "my \"big\" task"])
        ),
        "/d /s /c \"\"C:\\My Tools\\claude.cmd\" --resume abc-1 --name \"my big task\"\""
    );
}

#[test]
fn runs_a_js_override_with_the_given_node_runner() {
    let runner = NodeRunner {
        file: "electron.exe".into(),
        env: env(&[("ELECTRON_RUN_AS_NODE", "1")]),
    };
    let found = resolve(
        Platform::Windows,
        &[],
        &[("C:/fake.mjs", "")],
        Some("C:/fake.mjs"),
        Some(runner),
    );
    assert_eq!(
        found,
        Some(ClaudeBinary {
            file: "electron.exe".into(),
            prefix_args: strings(&["C:/fake.mjs"]),
            env: env(&[("ELECTRON_RUN_AS_NODE", "1")]),
            source: "C:/fake.mjs".into(),
            via_cmd: None,
        })
    );
}

#[test]
fn does_not_find_a_chosen_claude_that_is_not_there() {
    let chosen = Some("D:\\tools\\claude.exe");
    assert_eq!(
        resolve(Platform::Windows, &[("PATH", "C:\\npm")], &[], chosen, None),
        None
    );
    let found = resolve(
        Platform::Windows,
        &[("PATH", "C:\\npm")],
        &[("D:\\tools\\claude.exe", "")],
        chosen,
        None,
    );
    assert_eq!(found.unwrap().file, "D:\\tools\\claude.exe");
}

#[test]
fn returns_none_when_claude_is_missing() {
    assert_eq!(resolve(Platform::Windows, &[("PATH", "C:\\x")], &[], None, None), None);
}

/// Not in the TypeScript tests: the search on macOS and Linux, which the Electron version only
/// exercised on those systems.
#[test]
fn looks_on_the_path_and_where_installers_put_claude_on_macos_and_linux() {
    let found = resolve(
        Platform::Unix,
        &[("PATH", "/usr/bin:/opt/tools")],
        &[("/opt/tools/claude", "")],
        None,
        None,
    );
    assert_eq!(found.unwrap().file, "/opt/tools/claude");
    let found = resolve(
        Platform::Unix,
        &[("PATH", "/usr/bin")],
        &[("/usr/local/bin/claude", "")],
        None,
        None,
    );
    assert_eq!(found.unwrap().file, "/usr/local/bin/claude");
}

// ---- registry ----

fn this_process(pid: u32) -> bool {
    pid == std::process::id()
}

fn write_entry(home: &Path, pid: u32, session_id: &str) {
    let text = json!({ "pid": pid, "sessionId": session_id }).to_string();
    std::fs::write(home.join("sessions").join(format!("{pid}.json")), text).unwrap();
}

#[test]
fn parses_known_fields_and_ignores_unknown_status_values() {
    let entry = parse_registry_entry(&json!({
        "pid": 5, "sessionId": "abc", "cwd": "/w", "name": "n", "nameSource": "user", "status": "busy", "extra": 1
    }));
    assert_eq!(
        entry,
        Some(RegistryEntry {
            pid: 5,
            session_id: "abc".into(),
            cwd: Some("/w".into()),
            name: Some("n".into()),
            name_source: Some("user".into()),
            status: Some(RegistryStatus::Busy),
            started_at: None,
            proc_start: None,
        })
    );
    let odd = parse_registry_entry(&json!({ "pid": 5, "sessionId": "abc", "status": "thinking" }));
    assert_eq!(odd.unwrap().status, None);
    assert_eq!(parse_registry_entry(&json!({ "sessionId": "abc" })), None);
    assert_eq!(parse_registry_entry(&json!("nope")), None);
}

#[test]
fn tells_a_running_claude_from_an_entry_left_behind_under_an_id_another_program_has_now() {
    let pid = std::process::id();
    let entry = parse_registry_entry(&json!({ "pid": pid, "sessionId": "s-1", "procStart": "1000" })).unwrap();
    assert!(is_running_entry(&entry, &this_process, &|_| Some("1000".into())));
    assert!(!is_running_entry(&entry, &this_process, &|_| Some("2000".into())));
    // Where the system does not tell, or the entry does not say, the process id has to do.
    assert!(is_running_entry(&entry, &this_process, &|_| None));
    let unsaid = parse_registry_entry(&json!({ "pid": pid, "sessionId": "s-1" })).unwrap();
    assert!(is_running_entry(&unsaid, &this_process, &|_| Some("2000".into())));
    let gone = RegistryEntry {
        pid: 999_999_999,
        ..entry
    };
    assert!(!is_running_entry(&gone, &this_process, &|_| Some("1000".into())));

    let timed =
        parse_registry_entry(&json!({ "pid": 5, "sessionId": "a", "startedAt": 17, "procStart": "134" })).unwrap();
    assert_eq!(timed.started_at, Some(17.0));
    assert_eq!(timed.proc_start.as_deref(), Some("134"));
}

#[test]
fn forgets_the_entry_of_a_killed_claude_never_that_of_a_running_process() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("sessions")).unwrap();
    let pid = std::process::id();
    write_entry(home.path(), 999_999_999, "s-999999999");
    write_entry(home.path(), pid, &format!("s-{pid}"));
    forget_registry_entry(home.path(), 999_999_999, &this_process);
    forget_registry_entry(home.path(), pid, &this_process);
    let left: Vec<String> = std::fs::read_dir(home.path().join("sessions"))
        .unwrap()
        .map(|file| file.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(left, [format!("{pid}.json")]);
}

#[test]
fn reads_entries_by_pid_and_finds_transcripts() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir(home.join("sessions")).unwrap();
    write_entry(home, 77, "s-1");
    std::fs::write(home.join("sessions").join("78.json"), "{ broken").unwrap();
    assert_eq!(read_registry_entry(home, 77).unwrap().session_id, "s-1");
    assert_eq!(read_registry_entry(home, 78), None);
    assert_eq!(read_registry_entry(home, 79), None);

    std::fs::create_dir_all(home.join("projects").join("C--work")).unwrap();
    std::fs::write(home.join("projects").join("C--work").join("s-1.jsonl"), "").unwrap();
    assert!(transcript_exists(home, "s-1"));
    assert!(!transcript_exists(home, "s-2"));
}

/// The TypeScript has a second, non-blocking set of readers with this test of its own; here it
/// covers what that test adds: more than one project folder, and a home folder that is missing.
#[test]
fn reads_the_same_without_blocking_for_a_folder_behind_a_share() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir(home.join("sessions")).unwrap();
    write_entry(home, 77, "s-1");
    std::fs::write(home.join("sessions").join("78.json"), "{ broken").unwrap();
    assert_eq!(read_registry_entry(home, 77).unwrap().session_id, "s-1");
    assert_eq!(read_registry_entry(home, 78), None);
    assert_eq!(read_registry_entry(home, 79), None);

    std::fs::create_dir_all(home.join("projects").join("C--work")).unwrap();
    std::fs::create_dir_all(home.join("projects").join("C--other")).unwrap();
    std::fs::write(home.join("projects").join("C--work").join("s-1.jsonl"), "").unwrap();
    assert!(transcript_exists(home, "s-1"));
    assert!(transcript_modified(home, "s-1").is_some_and(|modified| modified > 1.0e12));
    assert!(!transcript_exists(home, "s-2"));
    assert!(!transcript_exists(&home.join("missing"), "s-1"));
}

#[test]
fn finds_an_entry_by_its_session_id_when_the_process_id_is_not_known() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    std::fs::create_dir(home.join("sessions")).unwrap();
    write_entry(home, 5740, "s-1");
    write_entry(home, 5741, "s-2");
    std::fs::write(home.join("sessions").join("notes.txt"), "s-2").unwrap();
    assert_eq!(find_registry_entry(home, "s-2", None).unwrap().pid, 5741);
    assert_eq!(find_registry_entry(home, "s-3", None), None);
    assert_eq!(find_registry_entry(&home.join("missing"), "s-1", None), None);
    // Told which entries are a running claude's, it passes over one left behind under a pid
    // another program has.
    write_entry(home, 5742, "s-2");
    let alive = |entry: &RegistryEntry| entry.pid == 5742;
    assert_eq!(find_registry_entry(home, "s-2", Some(&alive)).unwrap().pid, 5742);
    assert_eq!(find_registry_entry(home, "s-1", Some(&alive)), None);
}

// ---- hookSettings ----

#[test]
fn points_every_hook_at_the_local_server_with_the_token() {
    let settings = hook_settings(4000, "tok", "app 1");
    let hooks = settings["hooks"].as_object().unwrap();
    assert_eq!(
        hooks.keys().collect::<Vec<_>>(),
        ["UserPromptSubmit", "Stop", "Notification"]
    );
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0],
        json!({
            "type": "http",
            "url": "http://127.0.0.1:4000/hook/app%201/Stop",
            "headers": { "X-Reframe-Token": "tok" }
        })
    );
}
