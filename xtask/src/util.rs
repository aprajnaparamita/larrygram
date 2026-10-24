//! Shared helpers: workspace root, running commands, cargo metadata,
//! atomic file writes, and the test temp-dir helper.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Result type used across xtask; errors are human-readable messages.
pub type Result<T> = std::result::Result<T, String>;

/// The workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map(Path::to_path_buf).unwrap_or(manifest)
}

/// The cargo binary that invoked us, so toolchain overrides carry through.
pub fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

/// Runs `cmd`, inheriting stdio. Returns an error naming the command on failure.
pub fn run(cmd: &mut Command) -> Result<()> {
    let shown = format!("{cmd:?}");
    let status = cmd
        .current_dir(root())
        .status()
        .map_err(|e| format!("failed to start {shown}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} exited with {status}"))
    }
}

/// Runs `cmd` and captures stdout as a string.
pub fn output(cmd: &mut Command) -> Result<String> {
    let shown = format!("{cmd:?}");
    let out = cmd
        .current_dir(root())
        .output()
        .map_err(|e| format!("failed to start {shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{shown} exited with {}:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{shown} printed non-UTF-8: {e}"))
}

/// Returns true if `cargo <sub> --version` works, i.e. the subcommand is installed.
pub fn has_cargo_subcommand(sub: &str) -> bool {
    Command::new(cargo())
        .args([sub, "--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Writes `bytes` to `path` atomically: a temp file in the same
/// directory (unique per call, so concurrent writes never collide)
/// is renamed over `path` only after it is fully written.
pub fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    let tmp = unique_temp_path(path)?;
    {
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
        f.write_all(bytes)
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("cannot sync {}: {e}", tmp.display()))?;
    }
    finalize(&tmp, path)
}

/// A unique temp file name next to `dest` (pid + a per-process
/// counter + the file name), so concurrent runs never collide.
pub fn unique_temp_path(dest: &Path) -> Result<PathBuf> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = dest
        .file_name()
        .map(|n| n.to_os_string())
        .ok_or_else(|| format!("path has no file name: {}", dest.display()))?;
    let mut name = std::ffi::OsString::from(format!(".tmp.{}-{n}.", std::process::id()));
    name.push(file_name);
    name.push(".tmp");
    Ok(dest.with_file_name(name))
}

/// Renames `tmp` over `dest`, removing the temp file on failure.
pub fn finalize(tmp: &Path, dest: &Path) -> Result<()> {
    match std::fs::rename(tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(tmp);
            Err(format!(
                "cannot move {} -> {}: {e}",
                tmp.display(),
                dest.display()
            ))
        }
    }
}

/// Test-only temp directories. Every xtask test temp dir goes through
/// [`test_support::TempDir`], whose name carries the pid and a
/// counter so parallel test runs never delete each other's trees.
#[cfg(test)]
pub mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A unique temp directory, removed when it is dropped.
    pub struct TempDir(PathBuf);

    /// A unique temp path name: `larrygram-xtask-<label>-<pid>-<n>`,
    /// where the counter makes every call unique. The one naming
    /// helper for xtask test temp dirs.
    pub fn temp_path(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "larrygram-xtask-{label}-{}-{n}",
            std::process::id()
        ))
    }

    impl TempDir {
        /// Creates a fresh temp directory tagged with `label`.
        pub fn new(label: &str) -> Self {
            let path = temp_path(label);
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("test temp dir");
            TempDir(path)
        }

        /// The directory path.
        pub fn path(&self) -> &Path {
            &self.0
        }

        /// Joins a relative path under the directory.
        pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
            self.0.join(rel)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Copies a directory tree (files and subdirectories) from `src`
    /// into `dst`, creating `dst`. The fixture helper for the loader
    /// tests that mirror the committed `ui-spec/` data (and its icon
    /// set) in a temp workspace.
    pub fn copy_tree(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).expect("copy_tree destination");
        for entry in std::fs::read_dir(src).expect("copy_tree source") {
            let entry = entry.expect("copy_tree entry");
            let to = dst.join(entry.file_name());
            if entry.file_type().expect("copy_tree type").is_dir() {
                copy_tree(&entry.path(), &to);
            } else {
                std::fs::copy(entry.path(), &to).expect("copy_tree copy");
            }
        }
    }

    #[test]
    fn copy_tree_copies_files_and_subdirectories() {
        let src = TempDir::new("copy-tree-src");
        let dst = TempDir::new("copy-tree-dst");
        std::fs::create_dir_all(src.join("a/b")).unwrap();
        std::fs::write(src.join("a/b/c.txt"), "x").unwrap();
        copy_tree(src.path(), &dst.join("out"));
        assert_eq!(
            std::fs::read_to_string(dst.join("out/a/b/c.txt")).unwrap(),
            "x"
        );
    }

    #[test]
    fn temp_dirs_are_unique_per_call() {
        let a = TempDir::new("uniq");
        let b = TempDir::new("uniq");
        assert_ne!(a.path(), b.path());
        assert!(a.path().is_dir());
        assert!(b.path().is_dir());
    }
}

/// A workspace package from `cargo metadata`.
#[derive(Debug, Clone)]
pub struct Package {
    /// Package name.
    pub name: String,
    /// Directory containing Cargo.toml.
    pub dir: PathBuf,
    /// Names of direct dependencies (normal, dev and build).
    pub deps: Vec<String>,
}

/// Lists workspace packages via `cargo metadata --no-deps`.
pub fn workspace_packages() -> Result<Vec<Package>> {
    let json =
        output(Command::new(cargo()).args(["metadata", "--format-version", "1", "--no-deps"]))?;
    parse_metadata(&json)
}

/// Parses the JSON printed by `cargo metadata --no-deps`.
pub fn parse_metadata(json: &str) -> Result<Vec<Package>> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad cargo metadata JSON: {e}"))?;
    let pkgs = v
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or("cargo metadata: no `packages` array")?;
    let mut out = Vec::new();
    for p in pkgs {
        let name = p
            .get("name")
            .and_then(|n| n.as_str())
            .ok_or("cargo metadata: package without name")?;
        let manifest = p
            .get("manifest_path")
            .and_then(|m| m.as_str())
            .ok_or("cargo metadata: package without manifest_path")?;
        let dir = Path::new(manifest)
            .parent()
            .map(Path::to_path_buf)
            .ok_or("cargo metadata: manifest_path has no parent")?;
        let deps = p
            .get("dependencies")
            .and_then(|d| d.as_array())
            .map(|ds| {
                ds.iter()
                    .filter_map(|d| d.get("name").and_then(|n| n.as_str()))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        out.push(Package {
            name: name.to_owned(),
            dir,
            deps,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metadata() {
        let json = r#"{"packages":[{"name":"a","manifest_path":"/w/crates/a/Cargo.toml",
            "dependencies":[{"name":"b"},{"name":"serde"}]}]}"#;
        let pkgs = parse_metadata(json).unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].name, "a");
        assert_eq!(pkgs[0].dir, PathBuf::from("/w/crates/a"));
        assert_eq!(pkgs[0].deps, vec!["b", "serde"]);
    }

    #[test]
    fn rejects_bad_metadata() {
        assert!(parse_metadata("not json").is_err());
        assert!(parse_metadata("{}").is_err());
        assert!(parse_metadata(r#"{"packages":[{}]}"#).is_err());
    }
}
