//! Fuzz target for the `larrygram-ui-spec` loader (P1-08 goal 6).
//!
//! Feeds the input bytes to the loader's parse-and-validate path: the
//! first byte picks which of the data files is fuzzed, the rest of the
//! input is that file's content. The other files hold the
//! `tests/fixtures/good` documents, so cross-references can exercise
//! the whole pipeline (including the layout format v2 documents, task
//! P7-03). `Loader::load_all` must always answer (Ok or Err) —
//! libFuzzer reports any panic as a crash.

#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use larrygram_ui_spec::Loader;
use libfuzzer_sys::fuzz_target;

/// The data files, in loader read order.
const NAMES: [&str; 10] = [
    "panel-2016.yaml",
    "panel-365.yaml",
    "commands.yaml",
    "shortcuts.yaml",
    "layouts/panel-modern.yaml",
    "layouts/panel-classic.yaml",
    "themes/panel-modern.yaml",
    "themes/panel-classic.yaml",
    "icons.yaml",
    "contexts.yaml",
];

/// The baseline documents for the files not being fuzzed.
const BASELINE: [&str; 10] = [
    include_str!("../../tests/fixtures/good/ui-spec/panel-2016.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/panel-365.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/commands.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/shortcuts.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/layouts/panel-modern.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/layouts/panel-classic.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/themes/panel-modern.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/themes/panel-classic.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/icons.yaml"),
    include_str!("../../tests/fixtures/good/ui-spec/contexts.yaml"),
];

/// The icon SVG every baseline icon row names (the icon-file check
/// runs on disk).
const ICON_SVG: &str = include_str!("../../tests/fixtures/good/assets/icons/save.svg");

/// The process-unique fuzz workspace.
fn workspace() -> PathBuf {
    std::env::temp_dir().join(format!("larrygram-ui-spec-fuzz-{}", std::process::id()))
}

/// The shared `Loader` (compiles the schemas once per process).
fn loader() -> &'static Loader {
    static LOADER: OnceLock<Loader> = OnceLock::new();
    LOADER.get_or_init(|| {
        let root = workspace();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("ui-spec/layouts")).expect("fuzz workspace");
        std::fs::create_dir_all(root.join("ui-spec/themes")).expect("fuzz workspace");
        std::fs::create_dir_all(root.join("assets/icons")).expect("fuzz workspace");
        std::fs::write(root.join("assets/icons/save.svg"), ICON_SVG).expect("fuzz icon file");
        Loader::new(&root).expect("the embedded schemas always compile")
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((first, body)) = data.split_first() else {
        return;
    };
    // Creating the loader creates the fuzz workspace directory.
    let loader = loader();
    let fuzzed = usize::from(*first) % NAMES.len();
    for (i, name) in NAMES.iter().enumerate() {
        let bytes = if i == fuzzed { body } else { BASELINE[i].as_bytes() };
        std::fs::write(workspace().join("ui-spec").join(name), bytes).expect("fuzz input written");
    }
    let _ = loader.load_all();
});
