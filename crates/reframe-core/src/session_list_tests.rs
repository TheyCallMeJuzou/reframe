use super::*;

/// ms since 1970 of a UTC date (month from 1), the inverse of `utc_time`.
fn ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> f64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    ((days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60) * 1000) as f64
}

fn now() -> f64 {
    ms(2026, 11, 9, 15, 0)
}

fn en(time: f64) -> String {
    short_time(time, now(), Locale::En, &utc_time)
}

fn de(time: f64) -> String {
    short_time(time, now(), Locale::De, &utc_time)
}

#[test]
fn utc_time_and_back() {
    for (y, m, d, h, mi) in [
        (1970, 1, 1, 0, 0),
        (2024, 2, 29, 23, 59),
        (2026, 11, 9, 15, 0),
        (2000, 3, 1, 12, 30),
    ] {
        assert_eq!(
            utc_time(ms(y, m, d, h, mi)),
            LocalTime {
                year: y,
                month: m,
                day: d,
                hour: h,
                minute: mi
            }
        );
    }
}

#[test]
fn counts_minutes_hours_and_days_for_the_last_week() {
    assert_eq!(en(now() - 20_000.0), "now");
    assert_eq!(en(now() - 5.0 * MINUTE - 59_000.0), "5m");
    assert_eq!(en(now() - 59.0 * MINUTE), "59m");
    assert_eq!(en(now() - 2.0 * HOUR), "2h");
    assert_eq!(en(now() - 3.0 * DAY), "3d");
    assert_eq!(de(now() - 5.0 * MINUTE), "5 Min.");
    assert_eq!(de(now() - 3.0 * DAY), "3 Tg.");
}

#[test]
fn shows_the_date_for_older_ones_with_the_year_from_another_year() {
    assert_eq!(en(ms(2026, 10, 28, 0, 0)), "Oct 28");
    assert_eq!(de(ms(2026, 10, 28, 0, 0)), "28. Okt.");
    assert_eq!(en(ms(2025, 11, 6, 0, 0)), "Nov 2025");
}

#[test]
fn takes_a_time_a_little_ahead_as_now() {
    assert_eq!(en(now() + 5_000.0), "now");
}

#[test]
fn tells_the_exact_time() {
    assert_eq!(exact_time(now(), Locale::En, &utc_time), "Nov 9, 2026, 3:00 PM");
    assert_eq!(
        exact_time(ms(2026, 1, 2, 0, 5), Locale::En, &utc_time),
        "Jan 2, 2026, 12:05 AM"
    );
    assert_eq!(exact_time(now(), Locale::De, &utc_time), "09.11.2026, 15:00");
}

fn app(id: &str, window: Option<&str>, last: f64) -> AppListItem {
    AppListItem {
        id: id.into(),
        label: format!("label {id}"),
        cwd: format!("C:\\work\\{id}"),
        status: SessionStatus::Ended,
        lifecycle: Lifecycle::Ended,
        window_id: window.map(Into::into),
        window_number: window.map(|_| 1),
        last_active_at: last,
        elsewhere: false,
    }
}

#[test]
fn puts_sessions_in_a_window_first_then_the_most_recently_used() {
    let items = vec![
        app("a", None, 30.0),
        app("b", Some("w"), 10.0),
        app("c", None, 50.0),
        app("d", Some("w"), 20.0),
    ];
    let ids: Vec<String> = sort_app(items, "").into_iter().map(|item| item.id).collect();
    assert_eq!(ids, ["d", "b", "c", "a"]);
}

#[test]
fn searches_names_and_folders_whatever_the_case() {
    let items = vec![app("alpha", None, 1.0), app("beta", None, 2.0)];
    let ids: Vec<String> = sort_app(items.clone(), "  ALPHA ")
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert_eq!(ids, ["alpha"]);
    let ids: Vec<String> = sort_app(items, "work\\be").into_iter().map(|item| item.id).collect();
    assert_eq!(ids, ["beta"]);
    assert!(matches("", &[None]));
    assert!(!matches("x", &[None, Some("abc")]));
}

fn found(id: &str, modified: f64) -> FoundListItem {
    FoundListItem {
        claude_session_id: id.into(),
        cwd: "C:\\w".into(),
        title: None,
        modified_at: modified,
        distro: None,
    }
}

#[test]
fn merges_past_sessions_from_several_places_newest_first() {
    let (items, more) = merge_found(
        vec![
            (vec![found("a", 5.0), found("b", 1.0)], false),
            (vec![found("c", 3.0)], false),
        ],
        2,
    );
    let ids: Vec<&str> = items.iter().map(|item| item.claude_session_id.as_str()).collect();
    assert_eq!(ids, ["a", "c"]);
    assert!(more);
    let (_, more) = merge_found(vec![(vec![found("a", 5.0)], true)], 5);
    assert!(more);
    let (_, more) = merge_found(vec![(vec![found("a", 5.0)], false)], 5);
    assert!(!more);
}

fn running(id: &str) -> RunningListItem {
    RunningListItem {
        pid: 1,
        claude_session_id: id.into(),
        cwd: "C:\\w".into(),
        name: None,
        last_active_at: None,
        distro: None,
    }
}

#[test]
fn marks_entries_new_that_popped_up_since_the_first_list() {
    let mut fresh = FreshTracker::new(100.0);
    let first = SessionList {
        running: vec![running("r1")],
        found: vec![found("f1", 50.0)],
        ..SessionList::default()
    };
    // What was there at first is not new.
    assert!(!fresh.apply(&first));
    assert!(fresh.fresh().is_empty());
    let next = SessionList {
        running: vec![running("r1"), running("r2")],
        // Used since: new. Older, revealed by paging: not.
        found: vec![found("f1", 50.0), found("f2", 150.0), found("f3", 20.0)],
        ..SessionList::default()
    };
    assert!(fresh.apply(&next));
    let mut ids: Vec<&str> = fresh.fresh().iter().map(String::as_str).collect();
    ids.sort();
    assert_eq!(ids, ["f2", "r2"]);
    // Seen once, an entry is not new again.
    assert!(!fresh.apply(&next));
    fresh.clear();
    assert!(fresh.fresh().is_empty());
}

#[test]
fn keys_rows_by_session_or_conversation() {
    let list = SessionList {
        app: vec![app("a", None, 0.0)],
        running: vec![running("r")],
        found: vec![found("f", 0.0)],
        ..SessionList::default()
    };
    let keys: Vec<String> = list.refs().iter().map(ListItemRef::key).collect();
    assert_eq!(keys, ["app:a", "claude:r", "claude:f"]);
}

#[test]
fn names_a_folder_by_its_last_part() {
    assert_eq!(folder_name("C:\\work\\reframe\\"), "reframe");
    assert_eq!(folder_name("/home/me/x"), "x");
    assert_eq!(folder_name("C:"), "C:");
}
