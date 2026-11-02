//! The build id and the version line (task R11 goal 1).
//!
//! Every shipped build knows one short id that names its commit: the
//! About window shows `1.0.0-alpha.1 (abc1234)` and `larrygram-cli
//! --version` prints the same line. The id is resolved in this order:
//!
//! 1. the `LARRYGRAM_BUILD_SHA` environment variable of the running
//!    process (a caller that pins the id);
//! 2. `LARRYGRAM_BUILD_SHA` as it was set **at build time**
//!    (`release.yml` sets it from `GITHUB_SHA`, and the value is baked
//!    in when `larrygram-commands` compiles);
//! 3. `git rev-parse --short HEAD`, when git answers (a development
//!    build run from a checkout);
//! 4. `dev`.
//!
//! Resolution never fails and never panics (AGENTS.md section 6): any
//! missing or unusable source falls through to the next one. The
//! module lives in this crate (not in a build script) because the
//! About window (`crates/ui`) and the CLI are different packages and
//! cannot see one package's `cargo:rustc-env` (task R11 goal 1).

/// The version shown in the version line: the workspace version of
/// this crate (AGENTS.md: `[workspace.package] version`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The build id of a build that knows nothing better (task R11 goal 1).
pub const FALLBACK_ID: &str = "dev";

/// The longest build id shown: a short commit SHA (task R11 goal 1).
const MAX_ID_LEN: usize = 7;

/// The build id of this build (task R11 goal 1).
///
/// See the module documentation for the resolution order. Never
/// fails: an unknown id is [`FALLBACK_ID`].
pub fn build_id() -> String {
    resolve(
        runtime_build_sha(),
        option_env!("LARRYGRAM_BUILD_SHA"),
        git_short_sha(),
    )
}

/// The version line of this build: `<version> (<build id>)`, for
/// example `1.0.0-alpha.1 (abc1234)` (task R11 goal 1).
pub fn version_line() -> String {
    version_line_with(&build_id())
}

/// The version line with a given build id (task R11 goal 1: the fixed
/// id the About golden renders with).
pub fn version_line_with(build_id: &str) -> String {
    format!("{VERSION} ({build_id})")
}

/// The first usable candidate wins: runtime variable, then the value
/// baked in at build time, then git, else [`FALLBACK_ID`] (task R11
/// goal 1). Each candidate goes through [`shorten`].
fn resolve(runtime: Option<String>, compiled: Option<&str>, git: Option<String>) -> String {
    for candidate in [runtime, compiled.map(str::to_owned), git] {
        if let Some(id) = candidate.as_deref().and_then(shorten) {
            return id.to_owned();
        }
    }
    FALLBACK_ID.to_owned()
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

/// The `LARRYGRAM_BUILD_SHA` variable of the running process (task
/// R11 goal 1). Process environments do not exist on wasm.
#[cfg(not(target_arch = "wasm32"))]
fn runtime_build_sha() -> Option<String> {
    std::env::var("LARRYGRAM_BUILD_SHA").ok()
}

/// No process environment on wasm (task R11 goal 1).
#[cfg(target_arch = "wasm32")]
fn runtime_build_sha() -> Option<String> {
    None
}

/// `git rev-parse --short HEAD`, when git answers in the working
/// directory (task R11 goal 1). A missing git, a non-repository or a
/// broken pipe is `None`, never a panic (AGENTS.md section 6).
#[cfg(not(target_arch = "wasm32"))]
fn git_short_sha() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    sha_from(output.status.success(), &output.stdout)
}

/// No process on wasm (task R11 goal 1).
#[cfg(target_arch = "wasm32")]
fn git_short_sha() -> Option<String> {
    None
}

/// The trimmed stdout of a successful `git rev-parse`, else [`None`]
/// (task R11 goal 1).
fn sha_from(success: bool, stdout: &[u8]) -> Option<String> {
    if !success {
        return None;
    }
    Some(String::from_utf8_lossy(stdout).trim().to_owned())
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
    fn the_first_usable_candidate_wins() {
        assert_eq!(
            resolve(
                Some("runtime".to_owned()),
                Some("compile"),
                Some("gitsha1".to_owned())
            ),
            "runtime"
        );
        assert_eq!(
            resolve(None, Some("compile"), Some("gitsha1".to_owned())),
            "compile"
        );
        assert_eq!(resolve(None, None, Some("gitsha1".to_owned())), "gitsha1");
    }

    #[test]
    fn unusable_candidates_fall_through_and_the_last_resort_is_dev() {
        assert_eq!(
            resolve(
                Some("bad value".to_owned()),
                Some("compile"),
                Some("gitsha1".to_owned())
            ),
            "compile"
        );
        assert_eq!(resolve(Some(String::new()), None, None), FALLBACK_ID);
        assert_eq!(resolve(None, None, None), FALLBACK_ID);
    }

    #[test]
    fn sha_from_takes_the_stdout_of_a_successful_git() {
        assert_eq!(sha_from(true, b"abc1234\n"), Some("abc1234".to_owned()));
        assert_eq!(sha_from(false, b"abc1234\n"), None);
        assert_eq!(sha_from(true, b""), Some(String::new()));
    }

    #[test]
    fn the_version_line_is_the_version_and_the_id() {
        let line = version_line_with("abc1234");
        assert_eq!(line, format!("{VERSION} (abc1234)"));

        let line = version_line();
        let id = line
            .strip_prefix(&format!("{VERSION} ("))
            .and_then(|rest| rest.strip_suffix(')'))
            .unwrap_or_else(|| panic!("the version line is <version> (<id>): {line}"));
        assert!(!id.is_empty(), "the build id is never empty: {line}");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()), "{line}");
    }

    #[test]
    fn the_build_id_is_short_and_alphanumeric() {
        let id = build_id();
        assert!(!id.is_empty());
        assert!(id.len() <= MAX_ID_LEN.max(FALLBACK_ID.len()));
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
