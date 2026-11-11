//! Group colours and the automatic name/colour for new groups.
//!
//! Ported from the Electron version's `src/shared/groups.ts`.

/// A colour a group can have, and the message key of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupColor {
    pub hex: &'static str,
    pub label: &'static str,
}

pub const GROUP_COLORS: [GroupColor; 8] = [
    GroupColor {
        hex: "#d97757",
        label: "color.orange",
    },
    GroupColor {
        hex: "#5b9cf0",
        label: "color.blue",
    },
    GroupColor {
        hex: "#4fbf7f",
        label: "color.green",
    },
    GroupColor {
        hex: "#a67ff0",
        label: "color.purple",
    },
    GroupColor {
        hex: "#e06fa8",
        label: "color.pink",
    },
    GroupColor {
        hex: "#e0b04a",
        label: "color.yellow",
    },
    GroupColor {
        hex: "#3fbfbf",
        label: "color.teal",
    },
    GroupColor {
        hex: "#e06060",
        label: "color.red",
    },
];

pub fn is_group_color(value: &str) -> bool {
    GROUP_COLORS.iter().any(|color| color.hex == value)
}

/// The first colour no existing group uses; once all are taken, they repeat in order.
pub fn next_group_color<S: AsRef<str>>(used: &[S]) -> &'static str {
    let free = GROUP_COLORS
        .iter()
        .find(|color| !used.iter().any(|hex| hex.as_ref() == color.hex));
    free.unwrap_or(&GROUP_COLORS[used.len() % GROUP_COLORS.len()]).hex
}

/// The lowest number n for which `name_for(n)` is not taken yet ("Group 1", "Group 2", …).
pub fn next_group_number<S: AsRef<str>>(used: &[S], name_for: impl Fn(u32) -> String) -> u32 {
    let mut n = 1;
    while used.iter().any(|name| name.as_ref() == name_for(n)) {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_first_free_color_then_repeats() {
        assert_eq!(next_group_color::<&str>(&[]), GROUP_COLORS[0].hex);
        assert_eq!(
            next_group_color(&[GROUP_COLORS[0].hex, GROUP_COLORS[2].hex]),
            GROUP_COLORS[1].hex
        );
        let mut all: Vec<&str> = GROUP_COLORS.iter().map(|color| color.hex).collect();
        assert_eq!(next_group_color(&all), GROUP_COLORS[0].hex);
        all.push(all[0]);
        assert_eq!(next_group_color(&all), GROUP_COLORS[1].hex);
    }

    #[test]
    fn numbers_groups_by_the_lowest_free_name() {
        let name = |n: u32| format!("Group {n}");
        assert_eq!(next_group_number::<&str>(&[], name), 1);
        assert_eq!(next_group_number(&["Group 1", "Group 3", "API"], name), 2);
    }

    #[test]
    fn accepts_only_palette_colors() {
        assert!(is_group_color(GROUP_COLORS[3].hex));
        assert!(!is_group_color("#123456"));
    }
}
