//! Where reframe lives on the web.
//!
//! Ported from the Electron version's `src/shared/links.ts`.

pub const REPO_URL: &str = "https://github.com/TheyCallMeJuzou/reframe";
pub const AUTHOR_URL: &str = "https://github.com/TheyCallMeJuzou";
/// The newest release, with its installers and the portable ZIP.
pub const RELEASES_URL: &str = "https://github.com/TheyCallMeJuzou/reframe/releases/latest";

#[cfg(test)]
mod tests {
    use super::*;

    // The original builds it as `${REPO_URL}/releases/latest`; a `const` cannot, so this keeps the two in step.
    #[test]
    fn the_releases_are_under_the_repository() {
        assert_eq!(RELEASES_URL, format!("{REPO_URL}/releases/latest"));
    }

    #[test]
    fn the_links_are_the_ones_of_the_electron_version() {
        assert_eq!(REPO_URL, "https://github.com/TheyCallMeJuzou/reframe");
        assert_eq!(AUTHOR_URL, "https://github.com/TheyCallMeJuzou");
    }
}
