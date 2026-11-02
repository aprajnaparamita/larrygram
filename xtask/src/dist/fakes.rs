//! Test fakes shared by the platform pipeline tests (tasks R7 and
//! R9): a [`Sys`] that fabricates the built binaries.

use std::path::{Path, PathBuf};

use crate::util::Result;

use super::Sys;

pub use crate::util::test_support::TempDir;

/// A [`Sys`] that records every call and fabricates the release
/// binaries when `cargo build` runs. Any other command is an error, so
/// a test catches an unexpected tool call. The mac pipeline fabricates
/// its system tools itself and keeps its own fake.
pub struct FakeSys {
    /// The calls recorded so far, one line each.
    pub calls: Vec<String>,
    /// The release directory the fake `cargo` writes into.
    bin_dir: PathBuf,
    /// The binary file names the fake `cargo` writes there.
    names: &'static [&'static str],
}

impl FakeSys {
    /// A fake that writes the binary `names` into `bin_dir` when
    /// `cargo build` runs.
    pub fn new(bin_dir: &Path, names: &'static [&'static str]) -> Self {
        FakeSys {
            calls: Vec::new(),
            bin_dir: bin_dir.to_path_buf(),
            names,
        }
    }
}

impl Sys for FakeSys {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<()> {
        self.calls.push(format!("{program} {}", args.join(" ")));
        let name = Path::new(program)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| program.to_owned());
        if name != "cargo" {
            return Err(format!("unexpected command `{name}`"));
        }
        std::fs::create_dir_all(&self.bin_dir)
            .map_err(|e| format!("fake cargo: cannot create {}: {e}", self.bin_dir.display()))?;
        for bin in self.names {
            let path = self.bin_dir.join(bin);
            std::fs::write(&path, format!("fake binary {bin}"))
                .map_err(|e| format!("fake cargo: cannot write {}: {e}", path.display()))?;
        }
        Ok(())
    }
}
