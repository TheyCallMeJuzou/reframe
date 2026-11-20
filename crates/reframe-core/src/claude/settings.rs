//! Per-session settings file passed via `claude --settings`. Claude Code merges its hooks with
//! the user's and the project's own hooks. HTTP hooks report to reframe's local hook server.
//!
//! Ported from the Electron version's `src/main/claude/settings.ts`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

pub const HOOK_EVENTS: [&str; 3] = ["UserPromptSubmit", "Stop", "Notification"];
pub const HOOK_TOKEN_HEADER: &str = "X-Reframe-Token";

pub fn hook_settings(port: u16, token: &str, session_id: &str) -> Value {
    let mut hooks = Map::new();
    for event in HOOK_EVENTS {
        let url = format!(
            "http://127.0.0.1:{port}/hook/{}/{event}",
            encode_uri_component(session_id)
        );
        hooks.insert(
            event.to_owned(),
            json!([{ "hooks": [{ "type": "http", "url": url, "headers": { HOOK_TOKEN_HEADER: token } }] }]),
        );
    }
    json!({ "hooks": hooks })
}

pub fn write_hook_settings(dir: &Path, port: u16, token: &str, session_id: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let file = dir.join(format!("{session_id}.json"));
    let text = serde_json::to_string_pretty(&hook_settings(port, token, session_id))?;
    std::fs::write(&file, text)?;
    Ok(file)
}

/// As JavaScript's `encodeURIComponent`: everything but letters, digits and `- _ . ! ~ * ' ( )`
/// as percent-encoded UTF-8.
fn encode_uri_component(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
