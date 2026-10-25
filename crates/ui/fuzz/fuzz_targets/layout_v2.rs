//! Fuzz target for the v2 layout loader input (task P7-10, ADR 0065).
//!
//! The input bytes are one layout document, exactly what the v2 loader
//! reads into `ui-spec/layouts/*.yaml`. The target writes the document
//! into a scratch `ui-spec/` tree (the `tests/fixtures/good` baseline
//! for the other files) and runs the full staged load
//! (`Loader::load_all`: parse, nesting bound, schema validation,
//! decode and the cross-checks). Then it draws whatever loaded through
//! the Panel Classic and Panel Modern widgets for two frames each: the
//! chrome, the toolbars, every context menu, the command bar, the
//! Quick Access Toolbar and the File view.
//!
//! The load must always answer (`Ok` or `Err`), and a loaded layout
//! must never panic a widget: a panic is a crash, a hang is a finding.
//! The YAML parse itself is covered by `larrygram-ui-spec`'s
//! `loader_fuzz`; this target adds the widgets.

#![no_main]

use std::path::PathBuf;
use std::sync::OnceLock;

use larrygram_commands::app::{AppCtx, Settings};
use larrygram_commands::builtins;
use larrygram_ui::contexts::SelectionContext;
use larrygram_ui::panel_classic::{PanelClassic, PanelInput};
use larrygram_ui::panel_modern::{ListData, PanelModern};
use larrygram_ui::panel_modern::metrics::Metrics;
use larrygram_ui_spec::v2::LayoutKind;
use larrygram_ui_spec::{LayoutSets, Loader};
use libfuzzer_sys::fuzz_target;

/// The data files of one `ui-spec/` tree, in loader read order. The
/// two layout slots take the fuzzed document; the rest are the
/// baseline.
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

/// The baseline documents for the files that are not fuzzed.
const BASELINE: [&str; 10] = [
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/panel-2016.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/panel-365.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/commands.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/shortcuts.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/layouts/panel-modern.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/layouts/panel-classic.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/themes/panel-modern.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/themes/panel-classic.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/icons.yaml"),
    include_str!("../../../ui-spec/tests/fixtures/good/ui-spec/contexts.yaml"),
];

/// The layout slots the fuzzed document replaces.
const LAYOUT_SLOTS: [usize; 2] = [4, 5];

/// The icon SVG every baseline icon row names (the icon-file check
/// runs on disk).
const ICON_SVG: &str = include_str!("../../../ui-spec/tests/fixtures/good/assets/icons/save.svg");

/// The process-unique fuzz workspace.
fn workspace() -> PathBuf {
    std::env::temp_dir().join(format!("larrygram-ui-layout-v2-fuzz-{}", std::process::id()))
}

/// The shared `Loader` (the schemas compile once per process).
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

/// The builtin registry (built once per process).
fn registry() -> &'static larrygram_commands::Registry {
    static REGISTRY: OnceLock<larrygram_commands::Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| builtins::registry().expect("the builtin registry builds"))
}

/// Draws one loaded data set through the widgets for two frames: the
/// Panel Classic chrome, toolbars and every context menu, then the
/// Panel Modern command bar, Quick Access Toolbar and File view.
fn draw(sets: &LayoutSets) {
    let app = AppCtx::new(Settings::default());
    let lists = ListData::new();
    for kind in [LayoutKind::PanelClassic, LayoutKind::PanelModern] {
        let set = sets.get(kind);
        let ctx = egui::Context::default();
        let _ = larrygram_ui::theme::apply(&ctx, &set.theme);
        // `run_ui` may run the closure in several passes; the widget
        // state lives outside it.
        let classic = std::cell::RefCell::new(PanelClassic::new());
        let modern =
            std::cell::RefCell::new(PanelModern::with_metrics(Metrics::from_theme(&set.theme)));
        for frame in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let input = PanelInput::new(&set.v2, registry(), &app);
                let mut classic = classic.borrow_mut();
                if let Err(error) = classic.show(ui, &input) {
                    let _ = error;
                }
                for menu in &set.v2.context_menus {
                    classic.open_context_menu(menu.id.clone());
                    if let Err(error) = classic.context_menu(ui, &input, &menu.id, None) {
                        let _ = error;
                    }
                }
                let mut modern = modern.borrow_mut();
                if let Err(error) = modern.show(
                    ui,
                    &set.v2,
                    SelectionContext::ShapeSelected,
                    &app,
                    &lists,
                    registry(),
                ) {
                    let _ = error;
                }
                if let Err(error) = modern.show_qat(ui, &set.v2, &app, registry()) {
                    let _ = error;
                }
                if frame == 1 {
                    modern.set_file_view_open(true);
                }
                if let Err(error) = modern.show(
                    ui,
                    &set.v2,
                    SelectionContext::PictureSelected,
                    &app,
                    &lists,
                    registry(),
                ) {
                    let _ = error;
                }
            });
            // No renderer consumes the texture deltas here: clear them
            // before the output drops (epaint panics on unapplied
            // deltas).
            output.textures_delta.clear();
        }
    }
}

fuzz_target!(|data: &[u8]| {
    // Creating the loader creates the fuzz workspace directory.
    let loader = loader();
    for (index, name) in NAMES.iter().enumerate() {
        let bytes: &[u8] = if LAYOUT_SLOTS.contains(&index) {
            data
        } else {
            BASELINE[index].as_bytes()
        };
        std::fs::write(workspace().join("ui-spec").join(name), bytes)
            .expect("fuzz input written");
    }
    if let Ok(sets) = loader.load_all() {
        draw(&sets);
    }
});
