//! The Panel Modern File view (`docs/ui/panel-modern.md`, ADR 0061).
//!
//! File (or Alt+F) replaces the window below the title bar with this
//! view: a navigation column on the left with a ← back button, and the
//! active page on the right. Nav entries are pages (Info, New, Open,
//! Save As, Print, Export) or commands (Save, Close, Options): a
//! command runs and closes the view. Esc and ← close it and return to
//! the last tab.
//!
//! The pages show Larrygram's own content (ADR 0034 templates, the
//! settings of `AppCtx`); nothing here is Microsoft's.

use egui::{Button, Rect, Sense, Ui, UiBuilder, Vec2};
use larrygram_commands::AppCtx;
use larrygram_ui_spec::v2::{
    FileView as FileViewData, FileViewEntry, FileViewEntryKind, Item, ItemKind,
};

use crate::panel_modern::common::{Action, Resolve, finish, tooltip_text};
use crate::panel_modern::metrics::Metrics;
use crate::panel_modern::{ListData, ListEntries};
use crate::{Intent, UiError};

/// The id of the page shown when the layout names no default.
const DEFAULT_PAGE: &str = "file_view.info";

/// The File view state: whether it is open and which page it shows.
pub struct FileView {
    /// The File view covers the window.
    open: bool,
    /// The id of the active nav entry.
    page: String,
}

impl Default for FileView {
    fn default() -> Self {
        Self::new()
    }
}

impl FileView {
    /// A closed File view on the default page.
    pub fn new() -> Self {
        Self {
            open: false,
            page: DEFAULT_PAGE.to_owned(),
        }
    }

    /// Whether the File view covers the window.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the File view.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Opens or closes the File view.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// The id of the active nav entry.
    pub fn page(&self) -> &str {
        self.page.as_str()
    }

    /// Shows one page (a `file_view.*` nav id).
    pub fn set_page(&mut self, page: impl Into<String>) {
        self.page = page.into();
    }

    /// Draws the File view: the nav column and the active page.
    ///
    /// Returns the intents of the activations and whether the view
    /// must close (a command entry ran, or the user pressed Esc or ←).
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        view: &FileViewData,
        metrics: &Metrics,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
    ) -> Result<(Vec<Intent>, bool), UiError> {
        let mut out = Vec::new();
        let mut close = ui.input(|input| {
            input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::ArrowLeft)
        });
        let area = ui.available_rect_before_wrap();
        ui.allocate_rect(area, Sense::hover());

        // The navigation column.
        let nav = Rect::from_min_size(
            area.min,
            Vec2::new(metrics.file_view_nav_width, area.height()),
        );
        ui.painter().rect_filled(nav, 0.0, ui.visuals().panel_fill);
        let mut y = nav.min.y;
        if view.back_button {
            let rect = Rect::from_min_size(
                egui::pos2(nav.min.x, y),
                Vec2::new(nav.width(), metrics.file_view_nav_row_height),
            );
            let response = ui
                .push_id("file_view.back", |ui| {
                    ui.put(
                        rect,
                        Button::new(crate::panel_modern::common::arrow_atom(
                            12.0,
                            crate::panel_modern::common::Arrow::Left,
                        ))
                        .frame(false),
                    )
                })
                .inner
                .accessible_name("Back")
                .on_hover_text("Back");
            if response.clicked() {
                close = true;
            }
            y += metrics.file_view_nav_row_height;
        }
        for entry in &view.nav {
            let rect = Rect::from_min_size(
                egui::pos2(nav.min.x, y),
                Vec2::new(nav.width(), metrics.file_view_nav_row_height),
            );
            let selected = entry.id == self.page && entry.kind == FileViewEntryKind::Page;
            if selected {
                ui.painter()
                    .rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
            }
            let response = ui
                .push_id(entry.id.as_str(), |ui| {
                    ui.put(rect, Button::new(entry.label.as_str()).frame(false))
                })
                .inner
                .accessible_name(entry.label.as_str())
                .on_hover_text(entry.label.as_str());
            if response.clicked() {
                match entry.kind {
                    FileViewEntryKind::Command => {
                        if let Some(intent) = self.entry_intent(entry, resolve) {
                            out.push(intent);
                        }
                        close = true;
                    }
                    FileViewEntryKind::Page => {
                        self.set_page(entry.id.clone());
                    }
                }
            }
            y += metrics.file_view_nav_row_height;
        }

        // The page on the right.
        let page = Rect::from_min_size(
            egui::pos2(nav.max.x, area.min.y),
            Vec2::new((area.max.x - nav.max.x).max(0.0), area.height()),
        );
        ui.painter().rect_filled(page, 0.0, ui.visuals().panel_fill);
        let page_id = self.page.clone();
        ui.scope_builder(
            UiBuilder::new().max_rect(page.shrink(12.0)),
            |ui| -> Result<(), UiError> {
                self.show_page(ui, &page_id, app, lists, resolve, &mut out);
                Ok(())
            },
        )
        .inner?;
        Ok((out, close))
    }

    /// The intent of a nav command entry, when its command resolves.
    fn entry_intent(&self, entry: &FileViewEntry, resolve: Resolve<'_>) -> Option<Intent> {
        let item = item_with_command(
            entry.id.as_str(),
            entry.label.as_str(),
            entry.command.as_ref().map(|id| id.as_str()),
        );
        match resolve(&item) {
            Action::Emit(intent) => Some(intent),
            Action::Disabled { .. } => None,
        }
    }

    /// Draws one File view page (the entries of `docs/ui/panel-modern.md`).
    fn show_page(
        &mut self,
        ui: &mut Ui,
        page: &str,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) {
        match page {
            "file_view.new" => self.show_new(ui, lists, resolve, out),
            "file_view.open" => self.show_open(ui, app, resolve, out),
            "file_view.save_as" => self.show_save_as(ui, resolve, out),
            "file_view.print" => self.show_print(ui, resolve, out),
            "file_view.export" => self.show_export(ui, resolve, out),
            // Info, and any page the layout adds later.
            _ => self.show_info(ui, app),
        }
    }

    /// Info: document properties, the file path and the protection
    /// status.
    fn show_info(&self, ui: &mut Ui, app: &AppCtx) {
        ui.heading("Info");
        let (name, location, pages) = match app.active_document() {
            Some(session) => (
                session.display_name(),
                session
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "Not saved yet".to_owned()),
                session.document.pages.len(),
            ),
            None => ("Untitled".to_owned(), "Not saved yet".to_owned(), 0),
        };
        ui.label(format!("Name: {name}"));
        ui.label(format!("Location: {location}"));
        ui.label(format!("Pages: {pages}"));
        ui.label("Protection: none");
    }

    /// New: Larrygram's template tiles plus Blank Drawing.
    fn show_new(&self, ui: &mut Ui, lists: &ListData, resolve: Resolve<'_>, out: &mut Vec<Intent>) {
        ui.heading("New");
        ui.label("Templates");
        let tiles = match lists.entries("template_categories") {
            ListEntries::Empty => vec![NONE.to_owned()],
            ListEntries::Some(entries) => entries.to_vec(),
        };
        for tile in tiles {
            let placeholder = tile == NONE;
            let item = item_with_command("file_view.new.template", &tile, Some("file.new"));
            let action = resolve(&item);
            let enabled = matches!(action, Action::Emit(_)) && !placeholder;
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add(Button::new(tile.as_str()).min_size(Vec2::new(96.0, 48.0)))
                })
                .inner
                .accessible_name(tile.as_str());
            if placeholder {
                response.on_disabled_hover_text(NONE);
            } else {
                finish(response, &action, &tooltip_text(&tile, &item), out);
            }
        }
        let item = item_with_command(
            "file_view.new.blank",
            "Blank Drawing",
            Some("file.new.blank.drawing"),
        );
        let action = resolve(&item);
        let response = ui
            .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| {
                ui.add(Button::new("Blank Drawing").min_size(Vec2::new(96.0, 48.0)))
            })
            .inner
            .accessible_name("Blank Drawing");
        finish(
            response,
            &action,
            tooltip_text("Blank Drawing", &item).as_str(),
            out,
        );
    }

    /// Open: the recent files, then Browse.
    fn show_open(&self, ui: &mut Ui, app: &AppCtx, resolve: Resolve<'_>, out: &mut Vec<Intent>) {
        ui.heading("Open");
        ui.label("Recent files");
        if app.settings.recent_files.is_empty() {
            ui.add_enabled_ui(false, |ui| ui.label(NONE));
        }
        for (index, path) in app.settings.recent_files.iter().take(9).enumerate() {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let label = format!("{}  {}", index + 1, name);
            let item = item_with_command("file_view.open.recent", &label, Some("file.open"));
            let action = resolve(&item);
            let response = ui
                .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| {
                    ui.button(label.as_str())
                })
                .inner
                .accessible_name(label.as_str());
            finish(response, &action, &label, out);
        }
        let item = item_with_command("file_view.open.browse", "Browse…", Some("file.open"));
        let action = resolve(&item);
        let response = ui
            .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.button("Browse…"))
            .inner
            .accessible_name("Browse…");
        finish(
            response,
            &action,
            tooltip_text("Browse…", &item).as_str(),
            out,
        );
    }

    /// Save As: Browse.
    fn show_save_as(&self, ui: &mut Ui, resolve: Resolve<'_>, out: &mut Vec<Intent>) {
        ui.heading("Save As");
        let item = item_with_command("file_view.save_as.browse", "Browse…", Some("file.save_as"));
        let action = resolve(&item);
        let response = ui
            .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.button("Browse…"))
            .inner
            .accessible_name("Browse…");
        finish(
            response,
            &action,
            tooltip_text("Browse…", &item).as_str(),
            out,
        );
    }

    /// Print: the print settings and a preview placeholder.
    fn show_print(&self, ui: &mut Ui, resolve: Resolve<'_>, out: &mut Vec<Intent>) {
        ui.heading("Print");
        ui.label("Printer: Default");
        ui.label("Copies: 1");
        ui.label("Preview");
        let item = item_with_command("file_view.print.run", "Print", Some("file.print"));
        let action = resolve(&item);
        let response = ui
            .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.button("Print"))
            .inner
            .accessible_name("Print");
        finish(
            response,
            &action,
            tooltip_text("Print", &item).as_str(),
            out,
        );
    }

    /// Export: the export formats.
    fn show_export(&self, ui: &mut Ui, resolve: Resolve<'_>, out: &mut Vec<Intent>) {
        ui.heading("Export");
        for format in ["PNG", "SVG", "PDF"] {
            let item = item_with_command("file_view.export.format", format, Some("file.export"));
            let action = resolve(&item);
            let response = ui
                .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.button(format))
                .inner
                .accessible_name(format);
            finish(response, &action, tooltip_text(format, &item).as_str(), out);
        }
    }
}

/// The label of an empty list.
const NONE: &str = "(none)";

/// A minimal [`Item`] for the widgets: an id, a label and a command
/// id (the File view pages build their entries in code).
fn item_with_command(id: &str, label: &str, command: Option<&str>) -> Item {
    Item {
        id: id.to_owned(),
        kind: ItemKind::Button,
        label: Some(label.to_owned()),
        access: None,
        shortcut: None,
        command: command.map(larrygram_ui_spec::CommandId::new),
        new_command: false,
        fluent: None,
        visuicmd: None,
        icon: None,
        display: None,
        size: None,
        column: None,
        row: None,
        width: None,
        begin_group: false,
        hidden: false,
        dynamic_label: false,
        list: None,
        options: Vec::new(),
        items: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use larrygram_commands::{CommandId, Invocation, Source};

    /// A resolver that enables every command named in an item.
    fn always(item: &Item) -> Action {
        match &item.command {
            Some(command) => Action::Emit(Intent::new(
                CommandId::intern(command.as_str()).expect("a valid id"),
                Invocation::empty(Source::CommandBar),
            )),
            None => Action::Disabled {
                reason: "no command".to_owned(),
            },
        }
    }

    /// A minimal v2 file view.
    fn view() -> FileViewData {
        FileViewData {
            back_button: true,
            default_page: "file_view.info".to_owned(),
            nav: vec![
                FileViewEntry {
                    id: "file_view.info".to_owned(),
                    kind: FileViewEntryKind::Page,
                    label: "Info".to_owned(),
                    command: None,
                },
                FileViewEntry {
                    id: "file_view.save".to_owned(),
                    kind: FileViewEntryKind::Command,
                    label: "Save".to_owned(),
                    command: Some(larrygram_ui_spec::CommandId::new("file.save")),
                },
            ],
        }
    }

    #[test]
    fn starts_closed_on_the_default_page() {
        let file_view = FileView::new();
        assert!(!file_view.is_open());
        assert_eq!(file_view.page(), DEFAULT_PAGE);
    }

    #[test]
    fn open_and_close() {
        let mut file_view = FileView::new();
        file_view.open();
        assert!(file_view.is_open());
        file_view.set_open(false);
        assert!(!file_view.is_open());
        file_view.set_page("file_view.new");
        assert_eq!(file_view.page(), "file_view.new");
    }

    #[test]
    fn item_with_command_round_trips_the_command() {
        let item = item_with_command("a.b", "Label", Some("file.save"));
        assert_eq!(
            item.command.as_ref().map(|id| id.as_str()),
            Some("file.save")
        );
        assert_eq!(item.label.as_deref(), Some("Label"));
        let none = item_with_command("a.b", "Label", None);
        assert!(none.command.is_none());
        // An unresolvable command is disabled, never a panic.
        assert!(matches!(always(&none), Action::Disabled { .. }));
    }

    #[test]
    fn show_draws_the_nav_and_never_panics() {
        use crate::test_util::with_ui;
        let mut file_view = FileView::new();
        file_view.open();
        let app = AppCtx::new(larrygram_commands::Settings::default());
        let outcome = with_ui(|ui| {
            file_view.show(
                ui,
                &view(),
                &Metrics::default(),
                &app,
                &ListData::new(),
                &always,
            )
        });
        let (intents, close) = outcome.expect("the file view renders");
        assert!(intents.is_empty());
        assert!(!close);
    }

    #[test]
    fn the_default_impl_is_the_default_page() {
        let file_view = FileView::default();
        assert!(!file_view.is_open());
        assert_eq!(file_view.page(), DEFAULT_PAGE);
    }

    #[test]
    fn entry_intent_emits_only_when_the_command_resolves() {
        let file_view = FileView::new();
        let entry = FileViewEntry {
            id: "file_view.save".to_owned(),
            kind: FileViewEntryKind::Command,
            label: "Save".to_owned(),
            command: Some(larrygram_ui_spec::CommandId::new("file.save")),
        };
        let intent = file_view
            .entry_intent(&entry, &always)
            .expect("the command resolves");
        assert_eq!(intent.command.as_str(), "file.save");
        // An entry with no command resolves to nothing.
        let broken = FileViewEntry {
            id: "file_view.broken".to_owned(),
            kind: FileViewEntryKind::Command,
            label: "Broken".to_owned(),
            command: None,
        };
        assert!(file_view.entry_intent(&broken, &always).is_none());
    }

    #[test]
    fn the_pages_draw_with_live_data() {
        use larrygram_commands::app::{AppCtx, DocumentSession, Settings};
        use larrygram_model::Document;

        use crate::test_util::with_ui;
        let mut app = AppCtx::new(Settings::default());
        app.open_document(DocumentSession::new(Document::new(), None));
        app.settings
            .recent_files
            .push(std::path::PathBuf::from("/drawings/a.vsdx"));
        let mut lists = ListData::new();
        lists.insert("template_categories", vec!["Basic".to_owned()]);
        let mut file_view = FileView::new();
        file_view.open();
        for page in [
            "file_view.info",
            "file_view.new",
            "file_view.open",
            "file_view.save_as",
            "file_view.print",
            "file_view.export",
        ] {
            file_view.set_page(page);
            let outcome = with_ui(|ui| {
                file_view.show(ui, &view(), &Metrics::default(), &app, &lists, &always)
            });
            let (intents, close) = outcome.expect("the page renders");
            assert!(intents.is_empty(), "a draw-only frame dispatches nothing");
            assert!(!close, "{page} leaves the view open");
        }
    }
}
