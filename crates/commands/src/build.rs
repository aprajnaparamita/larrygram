//! The build id and the version line (task R11 goal 1, task R12
//! goal 1).
//!
//! Every shipped build knows one short id that names its commit: the
//! About window shows `1.0.0-alpha.1 (abc1234)` and `larrygram-cli
//! --version` prints the same line. The id is a **build-time
//! constant** (task R12 goal 1): the `LARRYGRAM_BUILD_SHA` variable
//! as it was set when this crate compiled (`release.yml` sets it from
//! `GITHUB_SHA`; `cargo xtask dist <platform>` sets it from
//! `git rev-parse --short HEAD` in the workspace root), shortened and
//! checked as below, else `dev`. No process is started and no
//! environment is read at run time: an installed app never runs git,
//! and a stray variable in the user's environment changes nothing.
//!
//! A plain `cargo build` sets no variable, so development builds read
//! `dev` (task R12 goal 3). The id and the version line are computed
//! once, not per frame (task R12 goal 1).
//!
//! The module lives in this crate (not in a build script) because the
//! About window (`crates/ui`) and the CLI are different packages and
//! cannot see one package's `cargo:rustc-env` (task R11 goal 1).

use std::sync::OnceLock;

/// The version shown in the version line: the workspace version of
/// this crate (AGENTS.md: `[workspace.package] version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build id of a build that knows nothing better (task R11 goal
/// 1): a plain development build.
pub const FALLBACK_ID: &str = "dev";

/// The longest build id shown: a short commit SHA (task R11 goal 1).
const MAX_ID_LEN: usize = 7;

/// The build id of this build (task R12 goal 1).
///
/// See the module documentation. Never fails: an unknown id is
/// [`FALLBACK_ID`]. A compile-time constant.
pub fn build_id() -> &'static str {
    resolve(option_env!("LARRYGRAM_BUILD_SHA"))
}

/// The version line of this build: `<version> (<build id>)`, for
/// example `1.0.0-alpha.1 (abc1234)` (task R11 goal 1). Computed
/// once, not per frame (task R12 goal 1).
pub fn version_line() -> &'static str {
    static LINE: OnceLock<String> = OnceLock::new();
    LINE.get_or_init(|| format!("{} ({})", VERSION, build_id()))
        .as_str()
}

/// The version line with a given build id (task R11 goal 1: the fixed
/// id the About golden renders with).
pub fn version_line_with(build_id: &str) -> String {
    format!("{VERSION} ({build_id})")
}

/// The build id from the compile-time variable: shortened and
/// checked, else [`FALLBACK_ID`] (task R12 goal 1).
fn resolve(compiled: Option<&'static str>) -> &'static str {
    compiled.and_then(shorten).unwrap_or(FALLBACK_ID)
}

/// Trims one candidate to a short, alphanumeric build id, or [`None`]
/// when it is empty or has other characters (task R11 goal 1).
fn shorten(candidate: &str) -> Option<&str> {
    let trimmed = candidate.trim();
    if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(trimmed.get(..MAX_ID_LEN).unwrap_or(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorten_trims_and_caps_one_candidate() {
        assert_eq!(shorten("abc1234"), Some("abc1234"));
        assert_eq!(shorten("abcdef1234567890"), Some("abcdef1"));
        assert_eq!(shorten("  abc1234\n"), Some("abc1234"));
        assert_eq!(shorten("ab"), Some("ab"));
    }

    #[test]
    fn shorten_refuses_empty_and_odd_candidates() {
        assert_eq!(shorten(""), None);
        assert_eq!(shorten(" \n"), None);
        assert_eq!(shorten("abc def"), None);
        assert_eq!(shorten("abc$"), None);
        assert_eq!(shorten("-abc123"), None);
    }

    #[test]
    fn resolve_shortens_the_variable_and_falls_back_to_dev() {
        assert_eq!(resolve(Some("abc1234")), "abc1234");
        assert_eq!(resolve(Some("abcdef1234567890")), "abcdef1");
        assert_eq!(resolve(Some("bad value")), FALLBACK_ID);
        assert_eq!(resolve(Some("")), FALLBACK_ID);
        assert_eq!(resolve(None), FALLBACK_ID);
    }

    #[test]
    fn a_plain_development_build_reads_dev() {
        // Task R12 goal 3: `cargo build` sets no variable.
        if option_env!("LARRYGRAM_BUILD_SHA").is_none() {
            assert_eq!(build_id(), FALLBACK_ID);
        }
    }

    #[test]
    fn the_build_id_is_the_variable_as_baked_in() {
        assert_eq!(
            build_id(),
            resolve(option_env!("LARRYGRAM_BUILD_SHA")),
            "the id never differs from the compile-time variable"
        );
        assert!(!build_id().is_empty());
        assert!(build_id().chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn the_version_line_is_the_version_and_the_id() {
        let line = version_line_with("abc1234");
        assert_eq!(line, format!("{VERSION} (abc1234)"));

        let shown = version_line();
        assert_eq!(shown, format!("{} ({})", VERSION, build_id()));
        assert!(shown.starts_with(VERSION), "{shown}");
        assert!(shown.ends_with(')'), "{shown}");
    }
}
