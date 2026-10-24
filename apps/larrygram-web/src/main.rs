//! Larrygram web app (wasm, built with trunk) (ADR 0030).
//!
//! The wasm twin of `apps/larrygram`: same shell, same registry, with
//! file upload and download instead of native dialogs (ADR 0036).
//! Settings persist in `localStorage` through the same eframe storage
//! slot (ADR 0032).
//!
//! `cargo build -p larrygram-web --target wasm32-unknown-unknown` is
//! the gate step; `trunk serve` runs it from `index.html`.

#![forbid(unsafe_code)]

use std::fmt;

/// A startup failure (one line; never a panic, AGENTS.md section 6).
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

/// The eframe app (the wasm run builds it; the host tests drive it).
#[cfg(any(target_arch = "wasm32", test))]
mod app {
    use eframe::egui;
    use larrygram_commands::{AppCtx, Settings};
    use larrygram_ui::settings::TextStore;
    use larrygram_ui::shell::Shell;
    use larrygram_ui_spec::Loader;

    use crate::AppError;

    /// The settings slot in the eframe storage (ADR 0032).
    pub(crate) const SETTINGS_KEY: &str = "settings.json";

    /// Builds the registry, loads both layouts and wraps them in the
    /// shell (ADR 0028, ADR 0031).
    pub(crate) fn build_shell(
        storage: Option<&dyn eframe::Storage>,
        registry: larrygram_commands::Registry,
        layouts: larrygram_ui_spec::LayoutSets,
    ) -> Result<Shell, AppError> {
        let mut slot = TextStore::new(storage.and_then(|storage| storage.get_string(SETTINGS_KEY)));
        let settings = super::settings_from(&mut slot)?;
        let mut app = AppCtx::new(settings);
        if let Some(warning) = slot.take_warning() {
            app.record_warning(warning);
        }
        Ok(Shell::new(registry, layouts, app))
    }

    /// The layouts of both command bars (the embedded `ui-spec` data).
    pub(crate) fn layouts() -> Result<larrygram_ui_spec::LayoutSets, AppError> {
        Loader::from_embedded().map_err(|error| AppError::new(error.to_string()))
    }

    /// The eframe app: one [`Shell`] plus the last persisted settings.
    pub(crate) struct WebApp {
        /// The UI shell (ADR 0029).
        pub(crate) shell: Shell,
        /// The settings last written to the store (the dirty check).
        pub(crate) saved_settings: Settings,
    }

    impl WebApp {
        /// Wraps a shell and remembers its settings.
        pub(crate) fn new(shell: Shell) -> Self {
            let saved_settings = shell.app.settings.clone();
            Self {
                shell,
                saved_settings,
            }
        }

        /// One UI pass: draw the frame and dispatch its intents.
        pub(crate) fn update_ui(&mut self, ui: &mut egui::Ui) {
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
        pub(crate) fn persist(&mut self, storage: &mut dyn eframe::Storage) {
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

    impl eframe::App for WebApp {
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
}

/// Loads the settings from a storage slot (ADR 0032): a corrupt slot
/// falls back to the defaults with one warning (ADR 0021).
#[cfg(any(target_arch = "wasm32", test))]
fn settings_from(
    slot: &mut larrygram_ui::settings::TextStore,
) -> Result<larrygram_commands::Settings, AppError> {
    larrygram_ui::settings::load_settings(slot).map_err(|error| AppError::new(error.to_string()))
}

/// The canvas element id in `index.html`.
#[cfg(target_arch = "wasm32")]
const CANVAS_ID: &str = "larrygram_canvas";

/// Builds the registry, loads both layouts and starts the eframe web
/// runner (ADR 0030, ADR 0031).
#[cfg(target_arch = "wasm32")]
async fn run() -> Result<(), AppError> {
    use wasm_bindgen::JsCast;

    let registry = larrygram_commands::builtins::registry()
        .map_err(|error| AppError::new(error.to_string()))?;
    let layouts = app::layouts()?;
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| AppError::new("no browser document"))?;
    let canvas = document
        .get_element_by_id(CANVAS_ID)
        .ok_or_else(|| AppError::new(format!("no #{CANVAS_ID} canvas")))?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|error| AppError::new(format!("#{CANVAS_ID} is not a canvas: {error:?}")))?;
    let web_options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(
            canvas,
            web_options,
            Box::new(move |cc| {
                larrygram_ui::install_image_loaders(&cc.egui_ctx);
                let shell = app::build_shell(cc.storage, registry, layouts)?;
                Ok(Box::new(app::WebApp::new(shell)))
            }),
        )
        .await
        .map_err(|error| AppError::new(format!("{error:?}")))
}

/// On the host the web app is a stub: it runs on wasm only.
#[cfg(not(target_arch = "wasm32"))]
fn run() -> Result<(), AppError> {
    Err(AppError::new("the web app runs on wasm32 only"))
}

/// Entry point of the wasm build.
#[cfg(target_arch = "wasm32")]
fn main() {
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = run().await {
            // Never crash: report on the browser console (AGENTS.md
            // section 6).
            web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!("error: {error}")));
        }
    });
}

/// Entry point on the host: the web app is wasm only.
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
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

    fn shell() -> larrygram_ui::shell::Shell {
        app::build_shell(
            None,
            larrygram_commands::builtins::registry().expect("registry"),
            app::layouts().expect("layouts"),
        )
        .expect("shell")
    }

    #[test]
    fn the_host_build_reports_it_runs_on_wasm_only() {
        let error = run().expect_err("no host run");
        assert!(error.to_string().contains("wasm32"), "{error}");
    }

    #[test]
    fn settings_round_trip_through_the_storage_slot() {
        let mut storage = MemoryStorage::default();
        let mut app = app::WebApp::new(shell());
        app.shell.app.settings.units = larrygram_commands::DisplayUnits::Feet;
        app.persist(&mut storage);
        let text = storage.get_string(app::SETTINGS_KEY).expect("stored");
        assert!(text.contains("Feet"), "{text}");

        let mut restored = app::WebApp::new(
            app::build_shell(
                Some(&storage),
                larrygram_commands::builtins::registry().expect("registry"),
                app::layouts().expect("layouts"),
            )
            .expect("shell"),
        );
        assert_eq!(
            restored.shell.app.settings.units,
            larrygram_commands::DisplayUnits::Feet
        );
        restored.persist(&mut storage);
    }

    #[test]
    fn a_corrupt_store_falls_back_with_one_warning() {
        let mut storage = MemoryStorage::default();
        storage.set_string(app::SETTINGS_KEY, "{not json".to_owned());
        let shell = app::build_shell(
            Some(&storage),
            larrygram_commands::builtins::registry().expect("registry"),
            app::layouts().expect("layouts"),
        )
        .expect("shell");
        assert_eq!(shell.app.settings, larrygram_commands::Settings::default());
        assert_eq!(shell.app.warnings.len(), 1, "one warning");
    }

    #[test]
    fn one_frame_draws_the_shell_without_crashing() {
        let mut app = app::WebApp::new(shell());
        let ctx = eframe::egui::Context::default();
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
}
