//! Release-tag version parsing, shared by the desktop updater and the CLI.
//!
//! Lives in its own module because the updater is `gui`-gated while the CLI is
//! not, and both channels must rank releases identically — a beta that the GUI
//! offers but `tokenguard update check` calls "up to date" is worse than either
//! behaviour alone.

/// Parse a release tag into a comparable version.
///
/// Tags are `vMAJOR.MINOR.PATCH` optionally followed by a prerelease, e.g.
/// `v0.2.0-beta.6`. A plain `(major, minor, patch)` triple cannot express those:
/// it fails to parse the `0-beta` segment, and even if it stripped the suffix it
/// would compare `beta.5` and `beta.6` as equal. Full semver ordering is what
/// makes the beta channel work — prereleases sort before their release
/// (`0.2.0-beta.6 < 0.2.0`) and among themselves (`beta.5 < beta.6`).
pub fn parse_release_version(s: &str) -> Option<semver::Version> {
    semver::Version::parse(s.trim().trim_start_matches('v')).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stable_tags_with_and_without_prefix() {
        assert_eq!(
            parse_release_version("v0.1.8"),
            Some(semver::Version::new(0, 1, 8))
        );
        assert_eq!(
            parse_release_version("0.1.8"),
            Some(semver::Version::new(0, 1, 8))
        );
    }

    #[test]
    fn parses_prerelease_tags() {
        // The whole reason this function exists: the old triple parser returned
        // None here, so every beta-channel check failed before comparing.
        let v = parse_release_version("v0.2.0-beta.6").expect("beta tag must parse");
        assert_eq!((v.major, v.minor, v.patch), (0, 2, 0));
        assert_eq!(v.pre.as_str(), "beta.6");
    }

    #[test]
    fn orders_prereleases_within_a_version() {
        // A triple comparison called these equal, suppressing every beta update.
        let b5 = parse_release_version("v0.2.0-beta.5").unwrap();
        let b6 = parse_release_version("v0.2.0-beta.6").unwrap();
        assert!(b6 > b5, "beta.6 must rank above beta.5");
        assert!(parse_release_version("v0.2.0-beta.10").unwrap() > b6);
    }

    #[test]
    fn prerelease_ranks_below_its_release() {
        let beta = parse_release_version("v0.2.0-beta.6").unwrap();
        let final_ = parse_release_version("v0.2.0").unwrap();
        assert!(final_ > beta, "0.2.0 must supersede 0.2.0-beta.6");
    }

    #[test]
    fn prerelease_ranks_above_the_previous_release() {
        // Someone on 0.1.8 who opts into betas must be offered 0.2.0-beta.6.
        assert!(
            parse_release_version("v0.2.0-beta.6").unwrap()
                > parse_release_version("v0.1.8").unwrap()
        );
    }

    #[test]
    fn rejects_unparseable_tags() {
        assert_eq!(parse_release_version("nightly"), None);
        assert_eq!(parse_release_version("v1.2"), None);
        assert_eq!(parse_release_version(""), None);
    }

    #[test]
    fn matches_the_shipped_crate_version() {
        // Guards against a bump that writes a tag shape this cannot rank, which
        // would silently disable update checks for that release.
        assert!(
            parse_release_version(env!("CARGO_PKG_VERSION")).is_some(),
            "the crate's own version must parse: {}",
            env!("CARGO_PKG_VERSION")
        );
    }
}
