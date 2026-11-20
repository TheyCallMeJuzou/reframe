//! Sessions that need the user (Claude asks for permission, or waits for an answer): which one
//! is next.
//!
//! Ported from the Electron version's `src/shared/attention.ts`.

use std::collections::HashSet;

/// The next session in `order` after `after` that is waiting, going round (`after` itself only
/// when no other one is); from the start of `order` without `after`. `None` when none is waiting.
pub fn next_waiting<'a>(order: &'a [String], waiting: &HashSet<String>, after: Option<&str>) -> Option<&'a str> {
    // As in JavaScript, an empty `after` counts as none; so does one that is not in `order`.
    let first = after
        .filter(|after| !after.is_empty())
        .and_then(|after| order.iter().position(|id| id == after))
        .map_or(0, |start| start + 1);
    (0..order.len())
        .filter_map(|step| order.get((first + step) % order.len()))
        .find(|id| waiting.contains(*id))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order() -> Vec<String> {
        ["a", "b", "c", "d"].map(String::from).to_vec()
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn goes_to_the_next_waiting_session_after_the_current_one_round_to_the_start() {
        let order = order();
        let waiting = set(&["b", "d"]);
        assert_eq!(next_waiting(&order, &waiting, None), Some("b"));
        assert_eq!(next_waiting(&order, &waiting, Some("b")), Some("d"));
        assert_eq!(next_waiting(&order, &waiting, Some("c")), Some("d"));
        assert_eq!(next_waiting(&order, &waiting, Some("d")), Some("b"));
    }

    #[test]
    fn stays_on_the_current_one_when_it_is_the_only_one_waiting_and_knows_when_none_is() {
        let order = order();
        assert_eq!(next_waiting(&order, &set(&["c"]), Some("c")), Some("c"));
        assert_eq!(next_waiting(&order, &set(&[]), Some("a")), None);
        assert_eq!(next_waiting(&[], &set(&["a"]), None), None);
        // A current one that is gone counts as none.
        assert_eq!(next_waiting(&order, &set(&["a"]), Some("x")), Some("a"));
    }
}
