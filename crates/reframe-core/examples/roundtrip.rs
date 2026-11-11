//! Reads a state.json and reports whether writing it back would change it. Nothing is written.
//! Usage: cargo run -p reframe-core --example roundtrip -- <state.json>

use reframe_core::state::AppState;
use serde_json::Value;

/// Paths at which the two documents differ, without the values (they may be private).
fn differences(path: &str, before: &Value, after: &Value, found: &mut Vec<String>) {
    match (before, after) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys().filter(|key| !a.contains_key(*key))) {
                let here = format!("{path}.{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => differences(&here, x, y, found),
                    (Some(_), None) => found.push(format!("{here}: dropped")),
                    (None, _) => found.push(format!("{here}: added")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                differences(&format!("{path}[{index}]"), x, y, found);
            }
        }
        _ if before != after => found.push(format!("{path}: changed")),
        _ => {}
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: roundtrip <state.json>");
    let text = std::fs::read_to_string(&path).expect("cannot read the file");
    let state = match AppState::parse(&text) {
        Ok(state) => state,
        Err(err) => {
            println!("{path}: {err}");
            std::process::exit(1);
        }
    };
    let written = state.to_json();
    let before: Value = serde_json::from_str(&text).unwrap();
    let after: Value = serde_json::from_str(&written).unwrap();
    println!(
        "{path}: {} sessions, {} windows, {} workspaces",
        state.sessions.len(),
        state.windows.len(),
        state.workspaces.len()
    );
    println!("same content: {}", before == after);
    println!("same text:    {}", written == text.replace("\r\n", "\n").trim_end());
    let mut found = Vec::new();
    differences("", &before, &after, &mut found);
    for line in found {
        println!("  {line}");
    }
}
