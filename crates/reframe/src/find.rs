//! Finding text in a terminal (its scrollback included) and web links in its lines.
//!
//! The Electron version had xterm.js' search and web-links addons for this.

use std::ops::Range;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

/// More matches than this are not counted ("1000+ found"), as in xterm.js.
pub const MAX_MATCHES: usize = 1000;

/// Which match to go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Next,
    Previous,
    /// The one the search is at, or the next one after it (while the text is being typed).
    Stay,
}

/// The text as a pattern that matches just it.
fn literal(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() * 2);
    for character in text.chars() {
        if r"\.+*?()|[]{}^$#&-~".contains(character) {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern
}

/// Every place the text is at, from the top of the scrollback down, without regard to case. One
/// more than [`MAX_MATCHES`] at most, which says that there are too many to count.
pub fn matches<T>(term: &Term<T>, text: &str) -> Vec<Match> {
    if text.is_empty() {
        return Vec::new();
    }
    // Without capitals the search does not mind the case.
    let Ok(mut pattern) = RegexSearch::new(&literal(&text.to_lowercase())) else {
        return Vec::new();
    };
    let start = Point::new(term.topmost_line(), Column(0));
    let end = Point::new(term.bottommost_line(), term.last_column());
    RegexIter::new(start, end, Direction::Right, term, &mut pattern)
        .take(MAX_MATCHES + 1)
        .collect()
}

/// The match to go to from a place (where the current match starts, or the top of what is
/// shown): the next or the previous one, around the ends.
pub fn pick(starts: &[Point], from: Point, step: Step) -> Option<usize> {
    if starts.is_empty() {
        return None;
    }
    Some(match step {
        Step::Stay => starts.iter().position(|start| *start >= from).unwrap_or(0),
        Step::Next => starts.iter().position(|start| *start > from).unwrap_or(0),
        Step::Previous => starts
            .iter()
            .rposition(|start| *start < from)
            .unwrap_or(starts.len() - 1),
    })
}

/// The web link at a column of a line, with the columns it covers. `line` has a character per
/// column.
pub fn link_at(line: &[char], column: usize) -> Option<(Range<usize>, String)> {
    let starts_here = |at: usize, text: &str| line[at..].iter().take(text.len()).copied().eq(text.chars());
    let mut at = 0;
    while at < line.len() {
        if !(starts_here(at, "https://") || starts_here(at, "http://")) {
            at += 1;
            continue;
        }
        let mut end = at;
        while end < line.len() && !line[end].is_whitespace() && !"\"'<>`{}|\\^".contains(line[end]) {
            end += 1;
        }
        // What a sentence or brackets put after a link is not part of it.
        while end > at && ".,:;!?)]'".contains(line[end - 1]) {
            end -= 1;
        }
        let link: String = line[at..end].iter().collect();
        let address = link.split_once("://").map_or("", |(_, rest)| rest);
        if (at..end).contains(&column) && !address.is_empty() {
            return Some((at..end, link));
        }
        at = end.max(at + 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::index::Line;

    use super::*;

    fn point(line: i32, column: usize) -> Point {
        Point::new(Line(line), Column(column))
    }

    #[test]
    fn the_next_and_previous_match_wrap_around() {
        let starts = [point(-5, 2), point(0, 0), point(0, 9), point(3, 1)];
        assert_eq!(pick(&starts, point(0, 0), Step::Next), Some(2));
        assert_eq!(pick(&starts, point(3, 1), Step::Next), Some(0));
        assert_eq!(pick(&starts, point(0, 0), Step::Previous), Some(0));
        assert_eq!(pick(&starts, point(-5, 2), Step::Previous), Some(3));
        // While typing, the match the search is at stays if it still matches.
        assert_eq!(pick(&starts, point(0, 0), Step::Stay), Some(1));
        assert_eq!(pick(&starts, point(0, 1), Step::Stay), Some(2));
        assert_eq!(pick(&starts, point(9, 0), Step::Stay), Some(0));
        assert_eq!(pick(&[], point(0, 0), Step::Next), None);
    }

    #[test]
    fn text_is_found_as_it_is_written() {
        assert_eq!(literal("a.b (c)"), r"a\.b \(c\)");
        assert_eq!(literal("plain"), "plain");
    }

    #[test]
    fn links_are_found_where_the_pointer_is() {
        let line: Vec<char> = "see https://example.com/a?b=1, or (http://x.org).".chars().collect();
        assert_eq!(link_at(&line, 0), None);
        assert_eq!(link_at(&line, 4), Some((4..29, "https://example.com/a?b=1".to_owned())));
        assert_eq!(
            link_at(&line, 28),
            Some((4..29, "https://example.com/a?b=1".to_owned()))
        );
        // The comma after it is not the link's.
        assert_eq!(link_at(&line, 29), None);
        assert_eq!(link_at(&line, 36), Some((35..47, "http://x.org".to_owned())));
        assert_eq!(link_at(&"https:// nothing".chars().collect::<Vec<_>>(), 2), None);
    }
}
