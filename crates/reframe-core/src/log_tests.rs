use super::*;

use std::time::Duration;

use serde_json::json;

fn temp() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("reframe-log-").tempdir().unwrap()
}

/// Whether the text starts like `\d{4}-\d\d-\d\dT`.
fn starts_with_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 11
        && bytes[..11].iter().enumerate().all(|(at, byte)| match at {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            _ => byte.is_ascii_digit(),
        })
}

#[test]
fn appends_one_line_per_event_with_level_and_data() {
    let root = temp();
    let dir = root.path().join("logs");
    let log = Logger::new(&dir);
    log.log(Level::Info, "session started", Some(json!({ "id": "a" })));
    log.info("second", None);
    let text = fs::read_to_string(log.file()).unwrap();
    let lines: Vec<&str> = text.trim().split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert!(starts_with_date(lines[0]), "{}", lines[0]);
    assert!(
        lines[0].ends_with(r#" INFO  session started {"id":"a"}"#),
        "{}",
        lines[0]
    );
}

#[test]
fn rotates_a_large_file() {
    let dir = temp();
    fs::write(dir.path().join("main.log"), "x".repeat(2 * 1024 * 1024 + 1)).unwrap();
    Logger::new(dir.path()).log(Level::Info, "fresh", None);
    assert!(
        fs::read_to_string(dir.path().join("main.log"))
            .unwrap()
            .contains("fresh")
    );
    assert!(fs::metadata(dir.path().join("main.old.log")).unwrap().len() > 2 * 1024 * 1024);
}

// Not in the Electron version, where the time and the padding are JavaScript's own.
#[test]
fn writes_the_time_as_javascript_does_and_pads_the_level() {
    let at = |millis: u64| iso_string(UNIX_EPOCH + Duration::from_millis(millis));
    assert_eq!(at(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(at(951_782_400_000), "2000-02-29T00:00:00.000Z");
    assert_eq!(at(1_794_355_199_999), "2026-11-10T23:59:59.999Z");
    assert_eq!(at(1_709_251_200_007), "2024-03-01T00:00:00.007Z");

    let dir = temp();
    let log = Logger::new(dir.path());
    log.warn("careful", None);
    log.error("broken", Some(json!({ "b": 1, "a": [true, null] })));
    let text = fs::read_to_string(log.file()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(&lines[0][24..], " WARN  careful");
    assert_eq!(&lines[1][24..], r#" ERROR broken {"b":1,"a":[true,null]}"#);
}
