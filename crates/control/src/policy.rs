//! What a remote caller may do (ADR 0081).
//!
//! Three checks, all made by the executor before a command runs:
//!
//! - **Remote access**: the command's own `Command::remote` declaration
//!   (task P9-03): `allowed`, `headless_only` (refused when attached, for
//!   example quitting the user's app) or `never`.
//! - **Paths**: every path argument and render output resolves (symbolic
//!   links followed; a new file by its parent directory) inside one of
//!   the policy roots.
//! - **Writes**: a written file must carry one of
//!   [`WRITE_EXTENSIONS`], and must not exist unless the call says
//!   `overwrite`.

use std::path::{Path, PathBuf};

use larrygram_commands::{CommandId, Registry};

use crate::error::ControlError;

/// The file extensions a remote caller may write (ADR 0081): drawings
/// and rendered images, nothing else.
pub const WRITE_EXTENSIONS: &[&str] = &["vsdx", "png", "svg"];

/// The path and access policy of one executor (ADR 0081).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// Directories a path argument must resolve inside. Empty refuses
    /// every path.
    pub roots: Vec<PathBuf>,
}

impl Policy {
    /// A policy with one root (the headless default: the working
    /// directory; the attached default: the user's home directory).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            roots: vec![root.into()],
        }
    }

    /// The same policy with one more root (`--allow-dir`).
    pub fn and_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.roots.push(root.into());
        self
    }

    /// Checks that `id` may run remotely (task P9-08): refuses a
    /// `never` command, and a `headless_only` one when `attached`.
    pub fn check_command(
        &self,
        registry: &Registry,
        id: &CommandId,
        attached: bool,
    ) -> Result<(), ControlError> {
        let _ = (registry, id, attached);
        Err(ControlError::not_implemented(
            "policy::Policy::check_command",
        ))
    }

    /// Checks a path the command will read (task P9-08) and returns it
    /// resolved. Outside every root is `Denied`.
    pub fn check_read(&self, path: &Path) -> Result<PathBuf, ControlError> {
        let _ = path;
        Err(ControlError::not_implemented("policy::Policy::check_read"))
    }

    /// Checks a file the command will write (task P9-08) and returns it
    /// resolved: inside a root, a [`WRITE_EXTENSIONS`] extension, and
    /// absent unless `overwrite`. Each failure is `Denied` with a reason
    /// the caller can act on.
    pub fn check_write(&self, path: &Path, overwrite: bool) -> Result<PathBuf, ControlError> {
        let _ = (path, overwrite);
        Err(ControlError::not_implemented("policy::Policy::check_write"))
    }

    /// Checks a directory the command will write files into (task
    /// P9-08; `file.export`): inside a root; the files it writes are
    /// checked by name with [`Policy::check_write`].
    pub fn check_write_dir(&self, path: &Path) -> Result<PathBuf, ControlError> {
        let _ = path;
        Err(ControlError::not_implemented(
            "policy::Policy::check_write_dir",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn roots_build_up_and_the_checks_are_stubs() {
        let policy = Policy::with_root("/tmp/a").and_root("/tmp/b");
        assert_eq!(
            policy.roots,
            [PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b")]
        );
        assert!(Policy::default().roots.is_empty());
        let registry = Registry::new();
        let id = CommandId::parse("app.quit").expect("id");
        let stub = |error: ControlError| assert_eq!(error.code(), ErrorCode::NotImplemented);
        stub(
            policy
                .check_command(&registry, &id, true)
                .expect_err("stub"),
        );
        stub(
            policy
                .check_read(Path::new("/tmp/a/x.vsdx"))
                .expect_err("stub"),
        );
        stub(
            policy
                .check_write(Path::new("/tmp/a/x.vsdx"), false)
                .expect_err("stub"),
        );
        stub(
            policy
                .check_write_dir(Path::new("/tmp/a"))
                .expect_err("stub"),
        );
        assert_eq!(WRITE_EXTENSIONS, ["vsdx", "png", "svg"]);
    }
}
