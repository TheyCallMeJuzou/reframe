use super::*;

/// A file as reframe 0.10 writes it, with every kind of node and every optional key.
const SAMPLE: &str = include_str!("../tests/data/state-0.10.json");

fn sample() -> String {
    // Checked out with CRLF line ends on Windows; the app writes LF.
    SAMPLE.replace("\r\n", "\n").trim_end().to_owned()
}

#[test]
fn a_file_from_reframe_0_10_is_written_back_unchanged() {
    let state = AppState::parse(&sample()).unwrap();
    assert_eq!(state.to_json(), sample());
}

#[test]
fn keeps_the_three_states_of_claude_name() {
    let state = AppState::parse(&sample()).unwrap();
    assert_eq!(state.sessions["s1"].claude_name, Some(None));
    assert_eq!(state.sessions["s2"].claude_name, None);
    let named = sample().replace(r#""claudeName": null"#, r#""claudeName": "kept""#);
    assert_eq!(
        AppState::parse(&named).unwrap().sessions["s1"].claude_name,
        Some(Some("kept".into()))
    );
}

#[test]
fn keeps_the_order_sessions_were_added_in() {
    let state = AppState::parse(&sample()).unwrap();
    assert_eq!(state.sessions.keys().collect::<Vec<_>>(), ["s1", "s2"]);
}

#[test]
fn an_older_file_gets_the_defaults_for_what_came_later() {
    let state = AppState::parse(r#"{"version":1,"sessions":{},"windows":[],"recentFolders":[]}"#).unwrap();
    assert_eq!(state.preferences, Preferences::default());
    assert!(state.workspaces.is_empty());
    assert_eq!(state.wsl_distros, None);

    let state = AppState::parse(
        r#"{"version":1,"sessions":{},"windows":[],"recentFolders":[],"preferences":{"nameFromFolder":false}}"#,
    )
    .unwrap();
    assert_eq!(
        state.preferences,
        Preferences {
            name_from_folder: false,
            ..Preferences::default()
        }
    );
}

#[test]
fn unknown_keys_are_dropped() {
    let state =
        AppState::parse(r#"{"version":1,"sessions":{},"windows":[],"recentFolders":[],"somethingNew":{"a":1}}"#)
            .unwrap();
    assert!(!state.to_json().contains("somethingNew"));
}

#[test]
fn a_file_from_a_newer_version_is_told_apart_from_a_broken_one() {
    assert_eq!(
        AppState::parse(r#"{"version":2,"anything":true}"#),
        Err(StateError::Newer(2))
    );
    assert!(matches!(AppState::parse("{ not json"), Err(StateError::Invalid(_))));
    assert!(matches!(
        AppState::parse(r#"{"version":0}"#),
        Err(StateError::Invalid(_))
    ));
}

#[test]
fn one_invalid_value_makes_the_whole_file_invalid() {
    for (from, to) in [
        (r#""fontSize": 15"#, r#""fontSize": 7"#),
        (r#""fontSize": 15"#, r#""fontSize": 13.5"#),
        (r#""language": "de""#, r#""language": "fr""#),
        (r#""closeWindow": "keep""#, r#""closeWindow": "never""#),
        (r#""permissionMode": "acceptEdits""#, r#""permissionMode": "yolo""#),
        (r#""lifecycle": "ended""#, r#""lifecycle": "paused""#),
        (r#""dir": "row""#, r#""dir": "diagonal""#),
        (r#""maximized": true"#, r#""maximized": "yes""#),
    ] {
        assert!(sample().contains(from), "the sample has no {from}");
        let broken = sample().replace(from, to);
        assert!(
            matches!(AppState::parse(&broken), Err(StateError::Invalid(_))),
            "{to} was accepted"
        );
    }
}

#[test]
fn a_split_needs_two_children() {
    let one_child = r#"{"version":1,"sessions":{},"recentFolders":[],"windows":[{"id":"w","bounds":null,
        "maximized":false,"hidden":false,"dock":[],"layout":{"kind":"split","id":"s","dir":"row",
        "children":[{"kind":"tile","sessionId":"a"}],"sizes":[100]}}]}"#;
    assert!(matches!(AppState::parse(one_child), Err(StateError::Invalid(_))));
}

#[test]
fn a_whole_font_size_may_be_written_with_a_fraction() {
    let state = AppState::parse(
        r#"{"version":1,"sessions":{},"windows":[],"recentFolders":[],"preferences":{"nameFromFolder":true,"fontSize":14.0}}"#,
    )
    .unwrap();
    assert_eq!(state.preferences.font_size, 14);
}

#[test]
fn a_first_start_has_one_empty_window() {
    let state = AppState::default();
    assert_eq!(state.windows.len(), 1);
    assert_eq!(AppState::parse(&state.to_json()).unwrap(), state);
}
