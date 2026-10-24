//! Larrygram desktop app (eframe/egui) (ADR 0030).
//!
//! The thin shell over `crates/ui` (L7, ADR 0002): it parses
//! `--ui-layout` (ADR 0032), builds the
//! [`Registry`](larrygram_commands::Registry) (ADR 0028), loads both
//! Panel layouts from the embedded `ui-spec` data (ADR 0031) and runs
//! the eframe event loop around
//! [`larrygram_ui::shell::Shell`].
//!
//! Settings persist in eframe's storage slot as `settings.json` (ADR
//! 0032); a corrupt slot falls back to the defaults with one warning
//! (ADR 0021). The app never panics on input (AGENTS.md section 6).

#![forbid(unsafe_code)]

use std::fmt;

use eframe::egui;
use larrygram_commands::{AppCtx, Settings};
use larrygram_ui::settings::TextStore;
use larrygram_ui::shell::Shell;
use larrygram_ui_spec::{LayoutKind, Loader};

/// A startup failure (one line; the app never panics, AGENTS.md
/// section 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    /// The formatted cause.
    pub message: String,
}

impl AppError {
    /// An error from one formatted line.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

/// The settings slot in the eframe storage (ADR 0032).
const SETTINGS_KEY: &str = "settings.json";

/// The app id and window title (eframe's `run_native` name).
const APP_NAME: &str = "larrygram";

/// The window-icon raster size in pixels (ADR 0067).
const ICON_PX: u32 = 128;

/// What the command line asked for (ADR 0032: `--ui-layout` wins over
/// the persisted setting).
#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    /// The Panel layout from `--ui-layout`, when given.
    ui_layout: Option<LayoutKind>,
    /// `--help`: print the usage line and exit.
    help: bool,
}

/// Parses the command line (`--ui-layout <layout>`, `--help`).
fn parse_args(args: &[String]) -> Result<Options, AppError> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let arg = args.get(index).map(String::as_str).unwrap_or_default();
        match arg {
            "--help" | "-h" => options.help = true,
            "--ui-layout" => {
                index += 1;
                let value = args.get(index).map(String::as_str).unwrap_or_default();
                options.ui_layout = Some(parse_layout(value)?);
            }
            other if other.starts_with("--ui-layout=") => {
                let value = other.get("--ui-layout=".len()..).unwrap_or_default();
                options.ui_layout = Some(parse_layout(value)?);
            }
            other => {
                return Err(AppError::new(format!(
                    "unknown option {other:?}; try --help"
                )));
            }
        }
        index += 1;
    }
    Ok(options)
}

/// The `--ui-layout` values (ADR 0032, task P7-09 Goal 2).
const LAYOUT_VALUES: &str = "panel-modern|panel-classic|panel-2016|panel-365";

/// The `--ui-layout` value: one of [`LAYOUT_VALUES`] (ADR 0032).
///
/// The pre-rename values (`2016`, `365`) are rejected like any
/// unknown value, with a message naming the current ones.
fn parse_layout(value: &str) -> Result<LayoutKind, AppError> {
    match value {
        "panel-modern" => Ok(LayoutKind::PanelModern),
        "panel-classic" => Ok(LayoutKind::PanelClassic),
        "panel-2016" => Ok(LayoutKind::Panel2016),
        "panel-365" => Ok(LayoutKind::Panel365),
        other => Err(AppError::new(format!(
            "unknown layout {other:?}; expected {LAYOUT_VALUES}"
        ))),
    }
}

/// The one-line usage, printed by `--help`.
fn help_text() -> &'static str {
    "usage: larrygram [--ui-layout panel-modern|panel-classic|panel-2016|panel-365] [--help]"
}

/// The app icon source: the single `icon.svg` brand asset (ADR 0067).
///
/// The web build serves the same file as the favicon
/// (`apps/larrygram-web/index.html`); no raster copy is committed.
fn window_icon_svg() -> &'static [u8] {
    include_bytes!("../../../crates/ui/assets/brand/icon.svg")
}

/// Rasterises one SVG to the RGBA window icon (ADR 0067).
///
/// The pixels are unmultiplied alpha, as [`egui::IconData`] asks.
/// A broken SVG is an error, never a panic (AGENTS.md section 6).
fn rasterise_icon(svg: &str, size: u32) -> Result<egui::IconData, AppError> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|error| AppError::new(format!("window icon: cannot parse icon.svg: {error}")))?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).ok_or_else(|| {
        AppError::new(format!(
            "window icon: cannot allocate a {size}x{size} pixmap"
        ))
    })?;
    let width = tree.size().width().max(1.0);
    let height = tree.size().height().max(1.0);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(size as f32 / width, size as f32 / height),
        &mut pixmap.as_mut(),
    );
    Ok(egui::IconData {
        rgba: unpremultiply(pixmap.data()),
        width: size,
        height: size,
    })
}

/// Converts premultiplied RGBA pixels to unmultiplied alpha.
fn unpremultiply(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premultiplied.len());
    for pixel in premultiplied.chunks_exact(4) {
        if let [r, g, b, a] = pixel {
            let (r, g, b, a) = (*r, *g, *b, *a);
            let scale = |channel: u8| -> u8 {
                if a == 0 {
                    0
                } else {
                    u8::try_from((u32::from(channel) * 255 + u32::from(a) / 2) / u32::from(a))
                        .unwrap_or(u8::MAX)
                }
            };
            out.extend_from_slice(&[scale(r), scale(g), scale(b), a]);
        }
    }
    out
}

/// The window icon from `icon.svg`, at [`ICON_PX`] (ADR 0067).
fn window_icon() -> Result<egui::IconData, AppError> {
    let svg = std::str::from_utf8(window_icon_svg())
        .map_err(|error| AppError::new(format!("window icon: icon.svg is not UTF-8: {error}")))?;
    rasterise_icon(svg, ICON_PX)
}

/// The eframe options with the app icon wired (ADR 0067).
///
/// A rasterisation failure is one warning line and the platform
/// default icon: the app still starts (AGENTS.md section 6).
fn native_options() -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default();
    match window_icon() {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(error) => eprintln!("warning: {error}"),
    }
    eframe::NativeOptions {
        viewport,
        ..eframe::NativeOptions::default()
    }
}

/// Builds the registry, loads both layouts and runs the eframe loop
/// (ADR 0030, ADR 0031).
fn run_with(args: &[String]) -> Result<(), AppError> {
    let options = parse_args(args)?;
    if options.help {
        println!("{}", help_text());
        return Ok(());
    }
    let registry = larrygram_commands::builtins::registry()
        .map_err(|error| AppError::new(error.to_string()))?;
    let layouts = Loader::from_embedded().map_err(|error| AppError::new(error.to_string()))?;
    let cli_layout = options.ui_layout;
    let native_options = native_options();
    eframe::run_native(
        APP_NAME,
        native_options,
        Box::new(move |cc| {
            larrygram_ui::install_image_loaders(&cc.egui_ctx);
            let shell = build_shell(cc.storage, registry, layouts, cli_layout)?;
            Ok(Box::new(LarrygramApp::new(shell)))
        }),
    )
    .map_err(|error| AppError::new(error.to_string()))
}

/// The shell for the app: settings from the storage slot (ADR 0032)
/// with the `--ui-layout` flag winning over the persisted value.
fn build_shell(
    storage: Option<&dyn eframe::Storage>,
    registry: larrygram_commands::Registry,
    layouts: larrygram_ui_spec::LayoutSets,
    cli_layout: Option<LayoutKind>,
) -> Result<Shell, AppError> {
    let mut slot = TextStore::new(storage.and_then(|storage| storage.get_string(SETTINGS_KEY)));
    let mut settings = larrygram_ui::settings::load_settings(&mut slot)
        .map_err(|error| AppError::new(error.to_string()))?;
    if let Some(kind) = cli_layout {
        // The flag wins over the persisted value (ADR 0032) and the
        // default layout stays in step (task P7-09 Goal 6).
        settings.ui_layout = kind;
        settings.default_layout = kind;
    }
    let mut app = AppCtx::new(settings);
    if let Some(warning) = slot.take_warning() {
        app.record_warning(warning);
    }
    Ok(Shell::new(registry, layouts, app))
}

/// The eframe app: one [`Shell`] plus the last persisted settings.
struct LarrygramApp {
    /// The UI shell (ADR 0029).
    shell: Shell,
    /// The settings last written to the store (the dirty check).
    saved_settings: Settings,
}

impl LarrygramApp {
    /// Wraps a shell and remembers its settings.
    fn new(shell: Shell) -> Self {
        let saved_settings = shell.app.settings.clone();
        Self {
            shell,
            saved_settings,
        }
    }

    /// One UI pass: draw the frame and dispatch its intents.
    fn update_ui(&mut self, ui: &mut egui::Ui) {
        let intents = match self.shell.show_in(ui) {
            Ok(intents) => intents,
            Err(error) => {
                self.shell.note_error(error.to_string());
                Vec::new()
            }
        };
        if let Err(error) = self.shell.dispatch(&intents) {
            self.shell.note_error(error.to_string());
        }
    }

    /// Writes the settings into the storage slot (ADR 0032).
    fn persist(&mut self, storage: &mut dyn eframe::Storage) {
        let mut slot = TextStore::new(storage.get_string(SETTINGS_KEY));
        if let Err(error) =
            larrygram_ui::settings::save_settings(&mut slot, &self.shell.app.settings)
        {
            self.shell.note_error(error.to_string());
            return;
        }
        if let Some(text) = slot.take_written() {
            storage.set_string(SETTINGS_KEY, text);
        }
    }
}

impl eframe::App for LarrygramApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.update_ui(ui);
        if self.shell.app.settings != self.saved_settings {
            if let Some(storage) = frame.storage_mut() {
                self.persist(storage);
            }
            self.saved_settings = self.shell.app.settings.clone();
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.persist(storage);
        self.saved_settings = self.shell.app.settings.clone();
    }
}

/// Entry point: prints `error: <message>` and exits non-zero on
/// failure (ADR 0026 style).
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = run_with(&args) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use eframe::Storage as _;

    /// A storage slot in memory (the eframe seam).
    #[derive(Default)]
    struct MemoryStorage {
        entries: BTreeMap<String, String>,
    }

    impl eframe::Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.entries.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.entries.insert(key.to_owned(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.entries.remove(key);
        }
        fn flush(&mut self) {}
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_command_line_parses_the_layout_flag() {
        // The four values of task P7-09 Goal 2, with and without `=`.
        for (value, kind) in [
            ("panel-modern", LayoutKind::PanelModern),
            ("panel-classic", LayoutKind::PanelClassic),
            ("panel-2016", LayoutKind::Panel2016),
            ("panel-365", LayoutKind::Panel365),
        ] {
            let options = parse_args(&strings(&["--ui-layout", value])).expect("args");
            assert_eq!(options.ui_layout, Some(kind), "{value}");
            assert!(!options.help);
            let options = parse_args(&strings(&[&format!("--ui-layout={value}")])).expect("args");
            assert_eq!(options.ui_layout, Some(kind), "--ui-layout={value}");
        }

        let options = parse_args(&[]).expect("args");
        assert_eq!(options, Options::default());
    }

    #[test]
    fn help_is_asked_for_and_printed() {
        let options = parse_args(&strings(&["--help"])).expect("args");
        assert!(options.help);
        assert!(help_text().contains("--ui-layout"));
    }

    #[test]
    fn a_bad_command_line_names_the_problem() {
        let error = parse_args(&strings(&["--nope"])).expect_err("unknown flag");
        assert!(error.to_string().contains("--nope"), "{error}");
        let error = parse_args(&strings(&["--ui-layout"])).expect_err("missing value");
        assert!(error.to_string().contains(LAYOUT_VALUES), "{error}");
        let error = parse_args(&strings(&["--ui-layout", "95"])).expect_err("bad layout");
        assert!(error.to_string().contains("95"), "{error}");
        let error = parse_args(&strings(&["--ui-layout=nope"])).expect_err("bad layout");
        let shown = error.to_string();
        assert!(shown.contains("nope"), "{shown}");
        assert!(shown.contains(LAYOUT_VALUES), "{shown}");
    }

    #[test]
    fn the_old_layout_values_are_rejected_naming_the_new_ones() {
        for old in ["2016", "365"] {
            let error = parse_args(&strings(&["--ui-layout", old])).expect_err("old value");
            let shown = error.to_string();
            assert!(shown.contains(old), "{shown}");
            assert!(shown.contains("panel-2016"), "{shown}");
            assert!(shown.contains("panel-365"), "{shown}");
        }
    }

    #[test]
    fn help_exits_before_the_event_loop() {
        run_with(&strings(&["--help"])).expect("--help runs no window");
    }

    #[test]
    fn a_bad_command_line_never_starts_the_event_loop() {
        let error = run_with(&strings(&["--oops"])).expect_err("unknown flag");
        assert!(error.to_string().contains("--oops"));
    }

    #[test]
    fn settings_round_trip_through_the_storage_slot() {
        let mut storage = MemoryStorage::default();
        let mut app = LarrygramApp::new(
            build_shell(
                Some(&storage),
                larrygram_commands::builtins::registry().expect("registry"),
                Loader::from_embedded().expect("layouts"),
                None,
            )
            .expect("shell"),
        );
        assert!(storage.entries.is_empty(), "nothing persisted yet");
        app.persist(&mut storage);
        let text = storage.get_string(SETTINGS_KEY).expect("stored");
        assert!(text.contains("ui_layout"), "{text}");

        let restored = build_shell(
            Some(&storage),
            larrygram_commands::builtins::registry().expect("registry"),
            Loader::from_embedded().expect("layouts"),
            Some(LayoutKind::Panel365),
        )
        .expect("shell");
        assert_eq!(
            restored.app.settings.ui_layout,
            LayoutKind::Panel365,
            "--ui-layout wins over the persisted setting"
        );
    }

    #[test]
    fn a_corrupt_store_falls_back_with_one_warning() {
        let mut storage = MemoryStorage::default();
        storage.set_string(SETTINGS_KEY, "{not json".to_owned());
        let shell = build_shell(
            Some(&storage),
            larrygram_commands::builtins::registry().expect("registry"),
            Loader::from_embedded().expect("layouts"),
            None,
        )
        .expect("shell");
        assert_eq!(shell.app.settings, Settings::default());
        assert_eq!(shell.app.warnings.len(), 1, "one warning");
    }

    #[test]
    fn a_missing_store_is_the_defaults_without_a_warning() {
        let shell = build_shell(
            None,
            larrygram_commands::builtins::registry().expect("registry"),
            Loader::from_embedded().expect("layouts"),
            None,
        )
        .expect("shell");
        assert_eq!(shell.app.settings, Settings::default());
        assert!(shell.app.warnings.is_empty());
    }

    #[test]
    fn one_frame_draws_the_shell_without_crashing() {
        let shell = build_shell(
            None,
            larrygram_commands::builtins::registry().expect("registry"),
            Loader::from_embedded().expect("layouts"),
            None,
        )
        .expect("shell");
        let mut app = LarrygramApp::new(shell);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                app.update_ui(ui);
            });
            output.textures_delta.clear();
        }
        // The frame records the per-layout state (task P7-09 Goal 3):
        // the last core tab is remembered and nothing else changes.
        assert_eq!(
            app.shell.app.settings.last_core_tab,
            Some("tab.home".to_owned())
        );
        let mut unchanged = app.shell.app.settings.clone();
        unchanged.last_core_tab = None;
        assert_eq!(app.saved_settings, unchanged);
    }

    #[test]
    fn saving_persists_the_settings() {
        let mut app = LarrygramApp::new(
            build_shell(
                None,
                larrygram_commands::builtins::registry().expect("registry"),
                Loader::from_embedded().expect("layouts"),
                None,
            )
            .expect("shell"),
        );
        let mut storage = MemoryStorage::default();
        app.shell.app.settings.show_grid = false;
        app.persist(&mut storage);
        assert!(storage.get_string(SETTINGS_KEY).is_some());
    }

    /// The `href="..."` value of one HTML line (test-side, no HTML
    /// parser in the tree).
    fn href_of(line: &str) -> Option<String> {
        let rest = line.split_once("href=\"")?.1;
        Some(rest.split_once('"')?.0.to_owned())
    }

    /// Reads the file `href` points at, relative to the web app's
    /// `index.html` (the trunk copy source).
    fn web_asset(href: &str) -> Vec<u8> {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let web_dir = manifest.join("..").join("larrygram-web");
        std::fs::read(web_dir.join(href)).expect("the favicon source reads")
    }

    #[test]
    fn the_window_icon_comes_from_the_brand_icon_svg() {
        // The embedded bytes are the brand asset, unchanged.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let brand = std::fs::read(
            manifest
                .join("..")
                .join("..")
                .join("crates/ui/assets/brand/icon.svg"),
        )
        .expect("the brand icon.svg reads");
        assert_eq!(window_icon_svg(), brand.as_slice());

        // The window icon is a rasterisation of those bytes.
        let icon = window_icon().expect("the icon rasterises");
        assert_eq!(icon.width, ICON_PX);
        assert_eq!(icon.height, ICON_PX);
        assert_eq!(
            icon.rgba.len(),
            (ICON_PX * ICON_PX * 4) as usize,
            "RGBA pixels, one per pixel"
        );
        assert!(
            icon.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "the raster is not fully transparent"
        );

        let options = native_options();
        let wired = options.viewport.icon.as_deref().expect("the icon is wired");
        assert_eq!(wired, &icon);
    }

    #[test]
    fn a_broken_icon_svg_is_an_error_not_a_panic() {
        let error = rasterise_icon("this is not svg", 32).expect_err("a broken SVG is an error");
        assert!(error.to_string().contains("icon.svg"), "{error}");
    }

    #[test]
    fn the_favicon_is_the_brand_icon_svg() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let html = std::fs::read_to_string(manifest.join("..").join("larrygram-web/index.html"))
            .expect("the web page reads");

        let icon_line = html
            .lines()
            .find(|line| line.contains("rel=\"icon\""))
            .expect("the page has a favicon line");
        assert!(
            icon_line.contains("type=\"image/svg+xml\""),
            "the favicon is an SVG: {icon_line}"
        );
        assert!(
            !icon_line.contains(".png"),
            "no raster favicon: {icon_line}"
        );
        let favicon = href_of(icon_line).expect("the favicon has an href");
        assert_eq!(favicon, "icon.svg");

        // Trunk copies that file from the one brand source.
        let copy_line = html
            .lines()
            .find(|line| line.contains("rel=\"copy\"") && line.contains("icon.svg"))
            .expect("the page copies icon.svg into the build");
        let source = href_of(copy_line).expect("the copy has an href");
        assert!(
            source.ends_with("crates/ui/assets/brand/icon.svg"),
            "the copy source is the brand asset: {source}"
        );
        assert_eq!(
            source.rsplit('/').next().unwrap_or_default(),
            favicon,
            "the copied file keeps the favicon's name"
        );
        assert_eq!(
            web_asset(&source),
            window_icon_svg(),
            "the favicon bytes are the brand icon.svg the window icon rasterises"
        );
    }
}
