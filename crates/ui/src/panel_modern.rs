//! The Panel Modern widgets: tab strip, groups, controls, Quick Access
//! Toolbar and File view (ADR 0061, `docs/ui/panel-modern.md`).
//!
//! [`PanelModern`] reads a [`LayoutV2`] and returns [`Intent`]s; the
//! widgets never execute commands themselves (ADR 0028). The tab
//! strip carries the File button and the upper-case tab captions; the
//! contextual tab sets (ADR 0064) appear when their context is
//! current. The command bar lays each group out as `stack` or `rows`
//! ([`place`]) and draws the controls at their sizes ([`controls`]).
//! The bar collapses (double-click, Ctrl+F1 or the ^ button) to the
//! tab strip, with the groups as an overlay while collapsed. The
//! Quick Access Toolbar and the File view are separate surfaces:
//! [`PanelModern::show_qat`] for the title bar, [`PanelModern::show`]
//! draws the File view instead of the chrome while it is open.
//!
//! Sizes come from [`metrics`] (the theme tokens); colours are egui
//! defaults until task P7-05's theme wiring merges.

pub mod common;
mod controls;
pub mod metrics;
pub mod place;

use std::collections::BTreeMap;

use egui::{Button, Layout, Rect, Sense, Ui, UiBuilder, Vec2};
use larrygram_commands::{AppCtx, Registry};
use larrygram_ui_spec::v2::{Item, ItemKind, LayoutV2, Tab};

use crate::{Intent, UiError};
use common::{Action, Resolve, Toggles, display_label, finish, tooltip_text};
use controls::Draw;
use metrics::Metrics;
use place::Placed;

/// The label of an empty run-time list (`docs/ui/layout-format.md`).
const NONE: &str = "(none)";

/// Run-time list contents the widgets draw (`list` fields).
///
/// The shell (P7-09) fills them from live data; an empty list draws
/// the "(none)" placeholder. The keys are the run-time list names of
/// `docs/ui/layout-format.md` (`recent_files`, `themes`, `colors`, ...).
#[derive(Debug, Default, Clone)]
pub struct ListData {
    /// List name to entries, in draw order.
    entries: BTreeMap<String, Vec<String>>,
}

/// The entries of one run-time list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListEntries<'a> {
    /// The list is empty: draw the "(none)" placeholder.
    Empty,
    /// The entries, in draw order.
    Some(&'a [String]),
}

impl ListData {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the entries of one list.
    pub fn insert(&mut self, name: impl Into<String>, entries: Vec<String>) {
        self.entries.insert(name.into(), entries);
    }

    /// The entries of one list.
    pub fn entries(&self, name: &str) -> ListEntries<'_> {
        match self.entries.get(name) {
            Some(entries) if !entries.is_empty() => ListEntries::Some(entries),
            _ => ListEntries::Empty,
        }
    }
}

/// The Panel Modern widget state (one per window; ADR 0032 rebuilds
/// it when the layout kind changes).
pub struct PanelModern {
    /// Index of the active core tab.
    active_core: usize,
    /// Id of the active contextual tab, when one is selected.
    active_contextual: Option<String>,
    /// The command bar is collapsed to the tab strip.
    collapsed: bool,
    /// The command bar shows as an overlay while collapsed.
    overlay_open: bool,
    /// The File view state.
    file_view: crate::file_view::FileView,
    /// Display-only toggle state (`check` and `toggle` controls).
    toggles: Toggles,
    /// Widget sizes.
    metrics: Metrics,
}

impl Default for PanelModern {
    fn default() -> Self {
        Self::new()
    }
}

impl PanelModern {
    /// A fresh Panel Modern: the first tab active, the command bar
    /// expanded, the File view closed.
    pub fn new() -> Self {
        Self::with_metrics(Metrics::default())
    }

    /// The same, with explicit widget sizes.
    pub fn with_metrics(metrics: Metrics) -> Self {
        Self {
            active_core: 0,
            active_contextual: None,
            collapsed: false,
            overlay_open: false,
            file_view: crate::file_view::FileView::new(),
            toggles: Toggles::new(),
            metrics,
        }
    }

    /// The widget sizes in use.
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Replaces the widget sizes (task P7-09 passes theme metrics).
    pub fn set_metrics(&mut self, metrics: Metrics) {
        self.metrics = metrics;
    }

    /// Whether the command bar is collapsed to the tab strip.
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Collapses or expands the command bar (Ctrl+F1, the ^ button).
    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed;
        if !collapsed {
            self.overlay_open = false;
        }
    }

    /// Whether the File view covers the window.
    pub fn is_file_view_open(&self) -> bool {
        self.file_view.is_open()
    }

    /// Opens or closes the File view (the File button).
    pub fn set_file_view_open(&mut self, open: bool) {
        self.file_view.set_open(open);
    }

    /// The File view state (page navigation).
    pub fn file_view(&self) -> &crate::file_view::FileView {
        &self.file_view
    }

    /// Draws the File button row, the tab strip and the command bar
    /// (or the File view while it is open), and returns one
    /// [`Intent`] per activation.
    ///
    /// `context` selects the contextual tabs (ADR 0064): task P7-08
    /// computes it once per frame and the shell passes it in.
    /// `lists` carries the run-time list contents; `registry` decides
    /// which controls dispatch and which render disabled (ADR 0031).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        layout: &LayoutV2,
        context: crate::contexts::SelectionContext,
        app: &AppCtx,
        lists: &ListData,
        registry: &Registry,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_with(ui, layout, context, app, lists, &|item| {
            common::action_for_item(item, registry, app)
        })
    }

    /// [`PanelModern::show`] with an injected command-state resolver
    /// (the test seam, as in `tab_bar.rs`).
    pub(crate) fn show_with(
        &mut self,
        ui: &mut Ui,
        layout: &LayoutV2,
        context: crate::contexts::SelectionContext,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        let mut out = Vec::new();
        if ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::F1)) {
            self.set_collapsed(!self.collapsed);
        }
        if self.file_view.is_open() {
            let Some(view) = layout.file_view.as_ref() else {
                self.file_view.set_open(false);
                return Ok(out);
            };
            let (intents, close) =
                self.file_view
                    .show(ui, view, &self.metrics, app, lists, resolve)?;
            out.extend(intents);
            if close {
                self.file_view.set_open(false);
            }
            return Ok(out);
        }
        let tabs = visible_tabs(layout, context);
        self.reconcile_tabs(&tabs);
        let strip = self.show_tab_strip(ui, &tabs, layout, &mut out)?;
        if self.collapsed {
            self.show_overlay(ui, &tabs, strip, app, lists, resolve, &mut out)?;
        } else {
            self.show_command_bar(ui, &tabs, app, lists, resolve, &mut out)?;
        }
        if !out.is_empty() {
            // "It closes again after a command runs" (panel-modern.md).
            self.overlay_open = false;
        }
        Ok(out)
    }

    /// Draws the Quick Access Toolbar (the left of the title bar).
    ///
    /// Save, Undo (a split with the history list; the label changes to
    /// "Undo <action>"), Redo ("Can't Redo" or "Repeat <action>") and
    /// the ▾ customise menu, which shows the current items as checked
    /// and disabled (`docs/ui/panel-modern.md`).
    pub fn show_qat(
        &mut self,
        ui: &mut Ui,
        layout: &LayoutV2,
        app: &AppCtx,
        registry: &Registry,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_qat_with(ui, layout, app, &|item| {
            common::action_for_item(item, registry, app)
        })
    }

    /// [`PanelModern::show_qat`] with an injected resolver.
    pub(crate) fn show_qat_with(
        &mut self,
        ui: &mut Ui,
        layout: &LayoutV2,
        app: &AppCtx,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        let mut out = Vec::new();
        ui.horizontal(|ui| -> Result<(), UiError> {
            for item in &layout.quick_access_toolbar {
                self.show_qat_item(ui, item, layout, app, resolve, &mut out)?;
            }
            Ok(())
        })
        .inner?;
        Ok(out)
    }

    /// One Quick Access Toolbar control.
    fn show_qat_item(
        &mut self,
        ui: &mut Ui,
        item: &Item,
        layout: &LayoutV2,
        app: &AppCtx,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        let label = display_label(item, app);
        let action = resolve(item);
        let button = Button::new(label.as_str()).small();
        let response = ui
            .push_id(item.id.as_str(), |ui| {
                ui.add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.add(button))
                    .inner
            })
            .inner
            .accessible_name(label.as_str());
        match item.kind {
            ItemKind::Split if !item.items.is_empty() || item.dynamic_label => {
                // Undo: the label part runs the command, the arrow
                // opens the history list.
                finish(response.clone(), &action, &tooltip_text(&label, item), out);
                let arrow = Button::new(common::arrow_atom(10.0, common::Arrow::Down)).small();
                let arrow_response = ui
                    .push_id((item.id.as_str(), "arrow"), |ui| {
                        ui.add_enabled_ui(matches!(action, Action::Emit(_)), |ui| ui.add(arrow))
                            .inner
                    })
                    .inner
                    .accessible_name(common::DROPDOWN_TIP)
                    .on_hover_text(common::DROPDOWN_TIP);
                if let Some(inner) =
                    egui::Popup::menu(&arrow_response).show(|ui| -> Result<(), UiError> {
                        let rows = match item.command.as_ref().map(|id| id.as_str()) {
                            Some("edit.undo") => undo_labels(app),
                            Some("edit.redo.or.repeat") => redo_labels(app),
                            _ => item
                                .items
                                .iter()
                                .map(|child| child.label.clone().unwrap_or_default())
                                .collect(),
                        };
                        show_label_rows(ui, &rows, &action, out);
                        Ok(())
                    })
                {
                    inner.inner?;
                }
            }
            ItemKind::Dropdown => {
                // The ▾ customise menu: the current items, checked and
                // disabled (customisation is out of scope).
                finish(response.clone(), &action, &tooltip_text(&label, item), out);
                // The customise menu is display-only: the current
                // items, checked and disabled.
                let _ = egui::Popup::menu(&response).show(|ui| {
                    for entry in &layout.quick_access_toolbar {
                        let name = entry.label.clone().unwrap_or_default();
                        ui.add_enabled_ui(false, |ui| ui.selectable_label(true, name.as_str()));
                    }
                });
            }
            _ => finish(response, &action, &tooltip_text(&label, item), out),
        }
        Ok(())
    }

    /// Keeps the active tab valid: out-of-range indices clamp, and a
    /// contextual tab whose context ended returns the bar to the last
    /// core tab used (`docs/ui/panel-modern.md`).
    fn reconcile_tabs(&mut self, tabs: &[&Tab]) {
        let core_len = tabs.iter().take_while(|tab| tab.shown_in.is_none()).count();
        if let Some(id) = self.active_contextual.clone() {
            let still_visible = tabs.iter().any(|tab| tab.id == id);
            if !still_visible {
                self.active_contextual = None;
            }
        }
        if self.active_core >= core_len {
            self.active_core = core_len.saturating_sub(1);
        }
    }

    /// The index of the active tab in the visible tab list.
    fn active_index(&self, tabs: &[&Tab]) -> Option<usize> {
        if let Some(id) = &self.active_contextual {
            tabs.iter().position(|tab| &tab.id == id)
        } else {
            tabs.get(self.active_core).map(|_| self.active_core)
        }
    }

    /// Draws the tab strip: the File button, the tabs in upper case,
    /// the contextual tab sets and the collapse button while
    /// collapsed.
    fn show_tab_strip(
        &mut self,
        ui: &mut Ui,
        tabs: &[&Tab],
        layout: &LayoutV2,
        out: &mut Vec<Intent>,
    ) -> Result<Rect, UiError> {
        let m = self.metrics.clone();
        let width = ui.available_width();
        let (strip, strip_response) =
            ui.allocate_exact_size(Vec2::new(width, m.tab_strip_height), Sense::hover());

        // The File button.
        let file_label = layout
            .file_button
            .as_ref()
            .map(|button| button.label.clone())
            .unwrap_or_else(|| "File".to_owned());
        let file_rect =
            Rect::from_min_size(strip.min, Vec2::new(m.file_button_width, strip.height()));
        let response = ui
            .push_id("file_button", |ui| {
                ui.put(file_rect, Button::new(file_label.as_str()))
            })
            .inner
            .accessible_name(file_label.as_str())
            .on_hover_text(file_label.as_str());
        if response.clicked() {
            self.file_view.open();
        }

        // The tabs, left to right, with the contextual sets grouped.
        let mut x = file_rect.max.x;
        let active = self.active_index(tabs);
        for (index, tab) in tabs.iter().enumerate() {
            let caption = tab.label.clone().unwrap_or_default();
            let shown = caption.to_uppercase();
            let tab_width = text_width(ui, &shown) + 2.0 * m.tab_padding_x;
            let rect = Rect::from_min_size(
                egui::pos2(x, strip.min.y),
                Vec2::new(tab_width, strip.height()),
            );
            let contextual = tab.shown_in.is_some();
            if contextual {
                paint_contextual_tab(ui, rect, tab);
            }
            let response = ui
                .push_id(tab.id.as_str(), |ui| {
                    ui.put(
                        rect,
                        Button::new(shown.as_str())
                            .selected(active == Some(index))
                            .frame(true),
                    )
                })
                .inner
                .accessible_name(caption.as_str())
                .on_hover_text(caption.as_str());
            if response.double_clicked() {
                self.set_collapsed(!self.collapsed);
            } else if response.clicked() {
                if contextual {
                    self.active_contextual = Some(tab.id.clone());
                } else {
                    self.active_core = index;
                    self.active_contextual = None;
                }
                if self.collapsed {
                    self.overlay_open = true;
                }
            }
            x += tab_width;
        }

        // The tab-set captions, one per visible contextual set.
        self.paint_tab_set_captions(ui, tabs, strip, file_rect.max.x);

        // The mouse wheel over the strip moves between tabs.
        if strip_response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.cycle_tab(tabs, if scroll > 0.0 { 1 } else { -1 });
            }
        }

        // The collapse button while collapsed (expanded: in the bar).
        if self.collapsed {
            self.paint_collapse_button(ui, strip, "Expand the command bar", out);
        }
        Ok(strip)
    }

    /// Moves the active tab by `delta`, wrapping around the visible
    /// tabs (the mouse wheel over the tab strip).
    fn cycle_tab(&mut self, tabs: &[&Tab], delta: i32) {
        let len = tabs.len();
        if len == 0 {
            return;
        }
        let Some(current) = self.active_index(tabs) else {
            return;
        };
        let next = (current as i32 + delta).rem_euclid(len as i32) as usize;
        match tabs.get(next) {
            Some(tab) if tab.shown_in.is_some() => {
                self.active_contextual = Some(tab.id.clone());
            }
            _ => {
                self.active_contextual = None;
                self.active_core = next.min(len.saturating_sub(1));
            }
        }
    }

    /// Paints the coloured caption of each visible contextual tab set
    /// (the 4 px band and the set name; ADR 0064).
    fn paint_tab_set_captions(&self, ui: &Ui, tabs: &[&Tab], strip: Rect, start_x: f32) {
        let m = &self.metrics;
        let painter = ui.painter();
        let mut set_x = start_x;
        for tab in tabs.iter().filter(|tab| tab.shown_in.is_some()) {
            let caption = tab.label.clone().unwrap_or_default();
            let shown = caption.to_uppercase();
            let tab_width = text_width(ui, &shown) + 2.0 * m.tab_padding_x;
            let set_name = tab.tab_set.clone().unwrap_or_default();
            // The caption block sits above the tab, inside the strip:
            // a 4 px band across the top, then the set name.
            let band =
                Rect::from_min_size(egui::pos2(set_x, strip.min.y), Vec2::new(tab_width, 4.0));
            painter.rect_filled(band, 0.0, ui.visuals().selection.bg_fill);
            let caption_rect = Rect::from_min_size(
                egui::pos2(set_x, strip.min.y + 4.0),
                Vec2::new(tab_width, strip.height() - 4.0),
            );
            painter.text(
                caption_rect.left_top() + Vec2::new(2.0, 0.0),
                egui::Align2::LEFT_TOP,
                set_name,
                egui::FontId::proportional(9.0),
                ui.visuals().text_color(),
            );
            set_x += tab_width;
        }
    }

    /// The ^ / v button that collapses or expands the command bar.
    fn paint_collapse_button(
        &mut self,
        ui: &mut Ui,
        strip: Rect,
        name: &str,
        _out: &mut Vec<Intent>,
    ) {
        let m = &self.metrics;
        let size = m.qat_button;
        let rect = Rect::from_min_size(
            egui::pos2(strip.max.x - size.x - 2.0, strip.min.y + 2.0),
            size,
        );
        let glyph = if self.collapsed { "v" } else { "^" };
        let response = ui
            .push_id("collapse_command_bar", |ui| {
                ui.put(rect, Button::new(glyph).small())
            })
            .inner
            .accessible_name(name)
            .on_hover_text(name);
        if response.clicked() {
            self.set_collapsed(!self.collapsed);
        }
    }

    /// Draws the command bar: the active tab's groups, the separators
    /// and the ^ collapse button.
    fn show_command_bar(
        &mut self,
        ui: &mut Ui,
        tabs: &[&Tab],
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        let m = &self.metrics;
        let width = ui.available_width();
        let bar = ui
            .allocate_exact_size(Vec2::new(width, m.command_bar_height), Sense::hover())
            .0;
        ui.painter().line_segment(
            [
                egui::pos2(bar.min.x, bar.max.y),
                egui::pos2(bar.max.x, bar.max.y),
            ],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
        let inner = Rect::from_min_size(
            egui::pos2(bar.min.x, bar.min.y + m.command_bar_padding_top),
            Vec2::new(bar.width(), m.content_height() + m.group_caption_height),
        );
        if let Some(tab) = self
            .active_index(tabs)
            .and_then(|index| tabs.get(index).copied())
        {
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(egui::Align::TOP)),
                |ui| {
                    self.show_groups(ui, tab, app, lists, resolve, out)?;
                    Ok::<(), UiError>(())
                },
            )
            .inner?;
        }
        self.paint_collapse_button(ui, bar, "Collapse the command bar", out);
        Ok(())
    }

    /// Draws the groups of one tab left to right, separated by 1 px
    /// lines (`docs/ui/panel-modern.md`).
    fn show_groups(
        &mut self,
        ui: &mut Ui,
        tab: &Tab,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        for (index, group) in tab.groups.iter().enumerate() {
            if index > 0 {
                self.paint_separator(ui);
            }
            self.show_group(ui, group, app, lists, resolve, out)?;
        }
        Ok(())
    }

    /// The 1 px line between two groups.
    fn paint_separator(&self, ui: &mut Ui) {
        let m = &self.metrics;
        let cursor = ui.cursor().min;
        let height = m.content_height() + m.group_caption_height;
        ui.painter().line_segment(
            [
                egui::pos2(cursor.x, cursor.y),
                egui::pos2(cursor.x, cursor.y + height),
            ],
            egui::Stroke::new(
                m.group_separator_width,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ),
        );
    }

    /// Draws one group: the placed controls, the bottom caption and
    /// the corner launcher.
    fn show_group(
        &mut self,
        ui: &mut Ui,
        group: &larrygram_ui_spec::v2::Group,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        let m = self.metrics.clone();
        let items = place::visible_controls(group);
        let widths = measure_widths(ui, &items, &m);
        let placed = place::place(group, &widths, &m);
        let content_height = m.content_height();
        let group_height = content_height + m.group_caption_height;
        let group_width = placed
            .iter()
            .map(|p| p.rect.max.x)
            .fold(m.group_padding_x, f32::max)
            + m.group_padding_x;
        let rect = ui
            .allocate_exact_size(Vec2::new(group_width, group_height), Sense::hover())
            .0;

        for (item, p) in items.iter().zip(&placed) {
            let abs = Placed {
                rect: p.rect.translate(rect.min.to_vec2()),
                large: p.large,
            };
            let mut draw = Draw {
                lists,
                metrics: &m,
                app,
                toggles: &mut self.toggles,
            };
            controls::show_control(ui, item, abs, &mut draw, resolve, out, 0)?;
        }

        // The caption row: the label centred, the launcher in the
        // bottom-right corner.
        let caption = Rect::from_min_size(
            egui::pos2(rect.min.x, rect.min.y + content_height),
            Vec2::new(group_width, m.group_caption_height),
        );
        let text = group.label.clone().unwrap_or_default();
        ui.painter().text(
            caption.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(12.0),
            ui.visuals().widgets.noninteractive.fg_stroke.color,
        );
        if let Some(launcher) = &group.launcher {
            let item = launcher_item(group, launcher);
            let size = m.launcher_size;
            let launcher_rect =
                Rect::from_min_size(egui::pos2(caption.max.x - size.x, caption.min.y), size);
            let p = Placed {
                rect: launcher_rect,
                large: false,
            };
            let mut draw = Draw {
                lists,
                metrics: &m,
                app,
                toggles: &mut self.toggles,
            };
            controls::show_control(ui, &item, p, &mut draw, resolve, out, 0)?;
        }
        Ok(())
    }

    /// The command bar as an overlay under the tab strip while the bar
    /// is collapsed. It closes after a command runs or on a click
    /// elsewhere (`docs/ui/panel-modern.md`): an `Area`, not a popup,
    /// so a click on a tab still reaches the tab.
    #[allow(clippy::too_many_arguments)] // tabs, the strip rect and the drawing context
    fn show_overlay(
        &mut self,
        ui: &mut Ui,
        tabs: &[&Tab],
        strip: Rect,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        if !self.overlay_open {
            return Ok(());
        }
        let id = ui.make_persistent_id("panel_modern_command_bar_overlay");
        let anchor = egui::pos2(strip.min.x, strip.max.y);
        let frame = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
            .inner_margin(4.0);
        let response = egui::Area::new(id)
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                frame.show(ui, |ui| -> Result<(), UiError> {
                    let m = self.metrics.clone();
                    ui.scope_builder(
                        UiBuilder::new()
                            .layout(Layout::left_to_right(egui::Align::TOP))
                            .max_rect(ui.max_rect()),
                        |ui| {
                            ui.set_min_height(m.content_height() + m.group_caption_height);
                            if let Some(tab) = self
                                .active_index(tabs)
                                .and_then(|index| tabs.get(index).copied())
                            {
                                self.show_groups(ui, tab, app, lists, resolve, out)?;
                            }
                            Ok::<(), UiError>(())
                        },
                    )
                    .inner
                })
            });
        let overlay_rect = response.response.rect;
        response.inner.inner?;

        // A click outside the overlay and outside the tab strip closes
        // it ("on a click elsewhere"); Esc closes it too.
        let pointer = ui.input(|input| {
            let clicked = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::PointerButton { pressed: true, .. }));
            (
                clicked,
                input.pointer.latest_pos(),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if pointer.2 {
            self.overlay_open = false;
        }
        if let (true, Some(pos)) = (pointer.0, pointer.1)
            && !overlay_rect.contains(pos)
            && !strip.contains(pos)
        {
            self.overlay_open = false;
        }
        Ok(())
    }
}

/// The tabs a context shows: the core tabs, then the contextual tabs
/// whose `shown_in` matches (ADR 0064).
fn visible_tabs(layout: &LayoutV2, context: crate::contexts::SelectionContext) -> Vec<&Tab> {
    let id = context.id();
    layout
        .tabs
        .iter()
        .chain(
            layout
                .contextual_tabs
                .iter()
                .filter(|tab| tab.shown_in == Some(id)),
        )
        .collect()
}

/// The labels of the undoable edits (the QAT history list).
fn undo_labels(app: &AppCtx) -> Vec<String> {
    app.active_document()
        .map(|session| {
            session
                .undo
                .undo_labels()
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The labels of the redoable edits (the QAT history list).
fn redo_labels(app: &AppCtx) -> Vec<String> {
    app.active_document()
        .map(|session| {
            session
                .undo
                .redo_labels()
                .into_iter()
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Label rows of a popup (the QAT history lists).
fn show_label_rows(ui: &mut Ui, rows: &[String], action: &Action, out: &mut Vec<Intent>) {
    if rows.is_empty() {
        ui.add_enabled_ui(false, |ui| ui.label(NONE));
        return;
    }
    for row in rows {
        let response = ui
            .add_enabled_ui(matches!(action, Action::Emit(_)), |ui| {
                ui.button(row.as_str())
            })
            .inner;
        finish(response, action, row, out);
    }
}

/// The item view of a group launcher (the widgets draw [`Item`]s).
fn launcher_item(
    group: &larrygram_ui_spec::v2::Group,
    launcher: &larrygram_ui_spec::v2::Launcher,
) -> Item {
    Item {
        id: format!("{}.launcher", group.id),
        kind: ItemKind::Launcher,
        label: launcher.label.clone(),
        access: None,
        shortcut: None,
        command: Some(launcher.command.clone()),
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

/// The widths one control takes, by kind and size: combos use their
/// `width`, small controls the fixed icon width, the rest their label.
fn measure_widths(ui: &Ui, items: &[&Item], m: &Metrics) -> Vec<f32> {
    items
        .iter()
        .map(|item| match item.kind {
            ItemKind::Combo => (item.width.unwrap_or(107.0) as f32).clamp(16.0, 400.0),
            ItemKind::Check => {
                let label = item.label.clone().unwrap_or_default();
                m.check_size + 4.0 + text_width(ui, &label) + 4.0
            }
            _ => {
                let label = item.label.clone().unwrap_or_default();
                let text = text_width(ui, &label);
                if place::is_large(item) {
                    (text + 8.0).clamp(m.large_min_width, 3.0 * m.large_min_width)
                } else if item.size == Some(larrygram_ui_spec::v2::ControlSize::Small) {
                    m.small_width
                } else {
                    let icon = if item.icon.is_some() {
                        m.medium_icon + 4.0
                    } else {
                        0.0
                    };
                    text + icon + 12.0
                }
            }
        })
        .collect()
}

/// The width of `text` in the UI font, in logical pixels.
fn text_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        )
        .rect
        .width()
}

/// Paints the contextual-tab look of one tab: the set's fill behind
/// the caption (the band is painted by the set caption block).
fn paint_contextual_tab(ui: &Ui, rect: Rect, _tab: &Tab) {
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
}

#[cfg(test)]
mod tests;
