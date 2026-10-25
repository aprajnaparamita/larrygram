//! The Panel Modern widgets: tab strip, groups, controls, Quick Access
//! Toolbar and File view (ADR 0061, `docs/ui/panel-modern.md`).
//!
//! [`PanelModern`] reads a [`LayoutV2`] and returns [`Intent`]s; the
//! widgets never execute commands themselves (ADR 0028). The tab
//! strip carries the File button and the upper-case tab captions; the
//! contextual tab sets (ADR 0064) appear when their context is
//! current. The command bar lays each group out as `stack` or `rows`
//! ([`place`]) and draws the controls at their sizes (`controls`).
//! The bar collapses (double-click, Ctrl+F1 or the ^ button) to the
//! tab strip, with the groups as an overlay while collapsed. The
//! Quick Access Toolbar and the File view are separate surfaces:
//! [`PanelModern::show_qat`] for the title bar, [`PanelModern::show`]
//! draws the File view instead of the chrome while it is open.
//!
//! Sizes come from [`metrics`] (the theme tokens) and colours from
//! the theme by token name (ADR 0063, [`crate::theme::color_at`]).

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
use place::{GroupSize, Placed};

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

    /// The id of the active core tab (task P7-09 keeps it in
    /// `Settings::last_core_tab`).
    ///
    /// The active core tab is the `active_core`-nth visible core tab
    /// (no context id of its own), so the
    /// id is computed against the same visible list the tab strip
    /// draws.
    pub fn active_core_id(
        &self,
        layout: &LayoutV2,
        context: crate::contexts::SelectionContext,
    ) -> Option<String> {
        visible_tabs(layout, context)
            .iter()
            .take_while(|tab| tab.shown_in.is_none())
            .nth(self.active_core)
            .map(|tab| tab.id.clone())
    }

    /// Makes the core tab `id` active (task P7-09 restores
    /// [`Settings::last_core_tab`](larrygram_commands::Settings)).
    /// Answers `false` and changes nothing when the visible core tabs
    /// do not include `id`.
    pub fn set_active_core_id(
        &mut self,
        layout: &LayoutV2,
        context: crate::contexts::SelectionContext,
        id: &str,
    ) -> bool {
        let found = visible_tabs(layout, context)
            .iter()
            .take_while(|tab| tab.shown_in.is_none())
            .position(|tab| tab.id == id);
        match found {
            Some(index) => {
                self.active_core = index;
                self.active_contextual = None;
                true
            }
            None => false,
        }
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
    /// "Undo `<action>`"), Redo ("Can't Redo" or "Repeat `<action>`") and
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
        // A display-only menu opener (the customise dropdown) draws
        // enabled whatever its command says: the click opens its
        // menu, it does not run a command (P7-14's ruling on P7-10
        // finding 2, the Panel Classic split arrow's rule).
        let opens_display_menu = item.kind == ItemKind::Dropdown;
        let button = Button::new(label.as_str()).small();
        let response = ui
            .push_id(item.id.as_str(), |ui| {
                ui.add_enabled_ui(
                    matches!(action, Action::Emit(_)) || opens_display_menu,
                    |ui| ui.add(button),
                )
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
                // disabled (customisation is out of scope). The
                // button opens it even while `view.qat.customize` is
                // a stub (P7-14: opening a display-only menu is not
                // running a command).
                if matches!(action, Action::Emit(_)) {
                    finish(response.clone(), &action, &tooltip_text(&label, item), out);
                } else {
                    response.clone().on_hover_text(label.as_str());
                }
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
        // The strip's widgets sit at absolute rects inside it: a child
        // ui keeps their `put` calls from moving the parent's cursor
        // (each one added the item spacing, so the command bar started
        // 24 px below the strip; task P7-17).
        let mut strip_ui = ui.new_child(UiBuilder::new().max_rect(strip));
        let ui = &mut strip_ui;

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
                // The one accent-filled control of the window
                // (docs/ui/design.md).
                crate::theme::accent_controls(ui);
                ui.put(file_rect, Button::new(file_label.as_str()))
            })
            .inner
            .accessible_name(file_label.as_str())
            .on_hover_text(file_label.as_str());
        if response.clicked() {
            self.file_view.open();
        }

        // The tabs, left to right, with the contextual sets grouped.
        let slots = tab_slots(ui, tabs, &m, file_rect.max.x);
        let active = self.active_index(tabs);
        for (index, tab) in tabs.iter().enumerate() {
            let caption = tab.label.clone().unwrap_or_default();
            // Title case, as the data has it (task P7-18).
            let shown = caption.clone();
            let (x, tab_width) = slots.get(index).copied().unwrap_or((file_rect.max.x, 0.0));
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
                    let token = if active == Some(index) {
                        "tab.selected.text"
                    } else {
                        "tab.text"
                    };
                    let text = egui::RichText::new(shown.as_str())
                        .color(crate::theme::color_at(ui.ctx(), token));
                    // The selected tab shows by its accent caption and
                    // underline, not by the selected-button fill.
                    ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
                    // A contextual tab carries its tab-set name in
                    // front of its label (task P7-17: drawn over the
                    // label, the two were unreadable).
                    let atoms = match tab_set_caption(ui.ctx(), tab) {
                        Some(caption) => egui::Atoms::new((caption, text)),
                        None => egui::Atoms::new(text),
                    };
                    ui.put(
                        rect,
                        Button::new(atoms)
                            .selected(active == Some(index))
                            .frame(true),
                    )
                })
                .inner
                .accessible_name(caption.as_str())
                .on_hover_text(caption.as_str());
            if active == Some(index) {
                paint_tab_underline(ui, rect);
            }
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
        let slots = tab_slots(ui, tabs, m, start_x);
        for (index, tab) in tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.shown_in.is_some())
        {
            let (set_x, tab_width) = slots.get(index).copied().unwrap_or((start_x, 0.0));
            let set_name = tab.tab_set.clone().unwrap_or_default();
            let (band_token, _, _) = contextual_tokens(
                set_family(ui.ctx(), &set_name)
                    .as_deref()
                    .unwrap_or_default(),
            );
            // A 4 px band across the top of the tab; the set name is
            // the first atom of the tab's button ([`tab_set_caption`]).
            let band =
                Rect::from_min_size(egui::pos2(set_x, strip.min.y), Vec2::new(tab_width, 4.0));
            painter.rect_filled(band, 0.0, crate::theme::color_at(ui.ctx(), band_token));
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
            egui::Stroke::new(1.0, crate::theme::color_at(ui.ctx(), "chrome.border")),
        );
        let inner = Rect::from_min_size(
            egui::pos2(bar.min.x, bar.min.y + m.command_bar_padding_top),
            Vec2::new(bar.width(), m.content_height() + m.group_caption_height),
        );
        if let Some(tab) = self
            .active_index(tabs)
            .and_then(|index| tabs.get(index).copied())
        {
            // The groups leave the ^ button's corner free (task P7-17:
            // it drew over the last group).
            let room = (inner.width() - m.qat_button.x - 4.0).max(0.0);
            let mut groups = ui.new_child(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(egui::Align::TOP)),
            );
            groups.set_clip_rect(inner.intersect(ui.clip_rect()));
            self.show_groups(&mut groups, tab, app, lists, resolve, out, room)?;
        }
        self.paint_collapse_button(ui, bar, "Collapse the command bar", out);
        Ok(())
    }

    /// Draws the groups of one tab left to right, separated by 1 px
    /// lines (`docs/ui/panel-modern.md`), each at the size
    /// [`fit_group_sizes`] picks for `room` points of width.
    #[allow(clippy::too_many_arguments)] // the drawing context plus the room
    fn show_groups(
        &mut self,
        ui: &mut Ui,
        tab: &Tab,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
        room: f32,
    ) -> Result<(), UiError> {
        let m = self.metrics.clone();
        let sizes = fit_group_sizes(ui, &tab.groups, &m, room);
        for (index, group) in tab.groups.iter().enumerate() {
            if index > 0 {
                self.paint_separator(ui);
            }
            match sizes.get(index).copied().unwrap_or(GroupSize::Large) {
                GroupSize::Collapsed => {
                    self.show_collapsed_group(ui, group, app, lists, resolve, out)?;
                }
                size => {
                    let group = place::reduce(group, size, &m);
                    self.show_group(ui, &group, app, lists, resolve, out)?;
                }
            }
        }
        Ok(())
    }

    /// Draws one collapsed group: a large button with the group's
    /// caption that opens the whole group, at full size, as a popup
    /// (task P7-17). The popup closes after one of its commands runs,
    /// on a click outside it, or on Esc.
    fn show_collapsed_group(
        &mut self,
        ui: &mut Ui,
        group: &larrygram_ui_spec::v2::Group,
        app: &AppCtx,
        lists: &ListData,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        let m = self.metrics.clone();
        let width = collapsed_width(ui, group, &m);
        let rect = ui
            .allocate_exact_size(
                Vec2::new(width, m.content_height() + m.group_caption_height),
                Sense::hover(),
            )
            .0;
        let button = Rect::from_min_size(
            egui::pos2(rect.min.x + m.group_padding_x, rect.min.y),
            Vec2::new(
                (width - 2.0 * m.group_padding_x).max(0.0),
                m.content_height(),
            ),
        );
        // The button carries the group's caption, so the caption row
        // stays empty.
        let response = controls::collapsed_group_button(ui, group, button, &m);
        let popup_id = response.id.with("collapsed_group");
        let before = out.len();
        let shown = egui::Popup::menu(&response)
            .id(popup_id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| self.show_group(ui, group, app, lists, resolve, out));
        if let Some(inner) = shown {
            inner.inner?;
        }
        if out.len() > before {
            egui::Popup::close_id(ui.ctx(), popup_id);
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
                crate::theme::color_at(ui.ctx(), "group.separator"),
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
        let group_width = placed_width(&placed, &m);
        let rect = ui
            .allocate_exact_size(Vec2::new(group_width, group_height), Sense::hover())
            .0;
        // The controls sit at absolute rects inside the group: a child
        // ui keeps their `put` calls from moving the parent's cursor.
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
        let ui = &mut child;

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
        let caption = paint_caption(ui, group, rect, &m);
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
                                let room = ui.ctx().content_rect().width() - 8.0;
                                self.show_groups(ui, tab, app, lists, resolve, out, room)?;
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

/// The width a group's placed controls take, with the group padding
/// on both sides.
fn placed_width(placed: &[Placed], m: &Metrics) -> f32 {
    placed
        .iter()
        .map(|p| p.rect.max.x)
        .filter(|x| x.is_finite())
        .fold(m.group_padding_x, f32::max)
        + m.group_padding_x
}

/// The width `group` takes at one [`GroupSize`] stage.
fn group_width(ui: &Ui, group: &larrygram_ui_spec::v2::Group, size: GroupSize, m: &Metrics) -> f32 {
    if size == GroupSize::Collapsed {
        return collapsed_width(ui, group, m);
    }
    let group = place::reduce(group, size, m);
    let items = place::visible_controls(&group);
    let widths = measure_widths(ui, &items, m);
    let width = placed_width(&place::place(&group, &widths, m), m);
    // The caption and the launcher fit under the controls too.
    let caption = text_width(ui, group.label.as_deref().unwrap_or_default())
        + if group.launcher.is_some() {
            2.0 * m.launcher_size.x
        } else {
            0.0
        }
        + 2.0 * m.group_padding_x;
    width.max(caption)
}

/// The width of a collapsed group's button, with the group padding.
fn collapsed_width(ui: &Ui, group: &larrygram_ui_spec::v2::Group, m: &Metrics) -> f32 {
    let item = controls::collapsed_group_item(group);
    let widths = measure_widths(ui, &[&item], m);
    widths.first().copied().unwrap_or(m.large_min_width) + 2.0 * m.group_padding_x
}

/// The size of each group of a tab when the command bar has `room`
/// points (task P7-17, `docs/ui/panel-modern.md` "Narrow windows").
///
/// Every group starts at [`GroupSize::Large`]. While the groups do not
/// fit, the next stage of [`GroupSize::STAGES`] applies to one group
/// at a time, from the rightmost group leftwards; only when every
/// group has reached a stage does the next stage start. The result
/// is the first assignment that fits, or every group collapsed.
pub fn fit_group_sizes(
    ui: &Ui,
    groups: &[larrygram_ui_spec::v2::Group],
    m: &Metrics,
    room: f32,
) -> Vec<GroupSize> {
    let spacing = ui.spacing().item_spacing.x;
    let widths: Vec<[f32; 4]> = groups
        .iter()
        .map(|group| {
            [
                GroupSize::Large,
                GroupSize::Medium,
                GroupSize::Small,
                GroupSize::Collapsed,
            ]
            .map(|size| group_width(ui, group, size, m))
        })
        .collect();
    let total = |sizes: &[GroupSize]| -> f32 {
        let gaps = spacing * sizes.len().saturating_sub(1) as f32;
        sizes
            .iter()
            .zip(&widths)
            .map(|(size, widths)| widths.get(*size as usize).copied().unwrap_or(0.0))
            .sum::<f32>()
            + gaps
    };
    let mut sizes = vec![GroupSize::Large; groups.len()];
    if total(&sizes) <= room {
        return sizes;
    }
    'reduce: for stage in GroupSize::STAGES {
        for index in (0..sizes.len()).rev() {
            if let Some(size) = sizes.get_mut(index)
                && *size < stage
            {
                *size = stage;
            }
            if total(&sizes) <= room {
                break 'reduce;
            }
        }
    }
    // Give the room left back, left to right: a group grows one stage
    // at a time while everything still fits.
    for index in 0..sizes.len() {
        while let Some(bigger) = sizes.get(index).copied().and_then(larger) {
            let mut trial = sizes.clone();
            if let Some(size) = trial.get_mut(index) {
                *size = bigger;
            }
            if total(&trial) > room {
                break;
            }
            sizes = trial;
        }
    }
    sizes
}

/// The stage before `size` (one step larger), if any.
fn larger(size: GroupSize) -> Option<GroupSize> {
    match size {
        GroupSize::Large => None,
        GroupSize::Medium => Some(GroupSize::Large),
        GroupSize::Small => Some(GroupSize::Medium),
        GroupSize::Collapsed => Some(GroupSize::Small),
    }
}

/// Paints a group's caption, centred along the bottom of `rect`, and
/// returns the caption row.
fn paint_caption(ui: &Ui, group: &larrygram_ui_spec::v2::Group, rect: Rect, m: &Metrics) -> Rect {
    let caption = Rect::from_min_size(
        egui::pos2(rect.min.x, rect.min.y + m.content_height()),
        Vec2::new(rect.width(), m.group_caption_height),
    );
    ui.painter().text(
        caption.center(),
        egui::Align2::CENTER_CENTER,
        group.label.clone().unwrap_or_default(),
        crate::theme::font_at(ui.ctx(), "caption"),
        crate::theme::color_at(ui.ctx(), "group.caption.text"),
    );
    caption
}

/// Where each tab draws in the strip: `(x, width)` in tab order,
/// starting at `start_x`. The tab buttons and the contextual set
/// captions read the same table, so a caption block sits exactly
/// above its tab (P7-14).
fn tab_slots(ui: &Ui, tabs: &[&Tab], m: &Metrics, start_x: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::with_capacity(tabs.len());
    let mut x = start_x;
    for tab in tabs {
        let shown = tab.label.clone().unwrap_or_default();
        let caption = match tab.shown_in {
            Some(_) => {
                let name = tab.tab_set.clone().unwrap_or_default();
                ui.painter()
                    .layout_no_wrap(
                        name,
                        crate::theme::font_at(ui.ctx(), "caption"),
                        egui::Color32::WHITE,
                    )
                    .rect
                    .width()
                    + ui.spacing().icon_spacing.max(4.0)
            }
            None => 0.0,
        };
        let width = text_width(ui, &shown) + caption + 2.0 * m.tab_padding_x;
        out.push((x, width));
        x += width;
    }
    out
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
/// `width` (at least their label), small controls the icon width, the
/// rest the widest atom row the widget draws (the icon-over-label
/// stack of a large control, the icon-and-label row of a medium one,
/// the label-and-\u{25be} row of a large split's arrow part).
///
/// The constants are the frame and inter-atom spacing egui adds on
/// top of the glyph and icon widths (calibrated against the atoms'
/// preferred size, `panel_modern`'s tests). Under-measuring wraps the
/// label one letter per line (P7-10's finding);
/// `placement::every_drawn_control_fits_its_label` pins the rendered
/// side. The widgets render at exactly these widths
/// (`place::place`), so a caller can assert the fit.
pub fn measure_widths(ui: &Ui, items: &[&Item], m: &Metrics) -> Vec<f32> {
    items
        .iter()
        .map(|item| {
            let label = item.label.clone().unwrap_or_default();
            let text = text_width(ui, &label);
            let arrow = if matches!(
                item.kind,
                ItemKind::Dropdown | ItemKind::Gallery | ItemKind::Palette
            ) {
                ARROW_EXTRA
            } else {
                0.0
            };
            match item.kind {
                ItemKind::Combo => (item.width.unwrap_or(107.0) as f32)
                    .clamp(16.0, 400.0)
                    .max(text + COMBO_EXTRA),
                ItemKind::Check => m.check_size + 4.0 + text + CHECK_FRAME,
                _ => {
                    let split = matches!(item.kind, ItemKind::Split | ItemKind::SplitRecent);
                    if place::is_large(item) {
                        // The icon row and the label row (and, for a
                        // split, the label-and-\u{25be} row of the arrow
                        // part) all fit the column.
                        let below = text + if split { ARROW_EXTRA } else { arrow };
                        // The cap bounds a malformed label (400 px,
                        // the combo cap): inside it no word is split.
                        (m.large_icon.max(below) + LARGE_FRAME)
                            .clamp(m.large_min_width, MAX_LABEL_W)
                    } else if item.size == Some(larrygram_ui_spec::v2::ControlSize::Small) {
                        // Icon only, with the arrow part of a split
                        // beside it.
                        let arrow_part = if split { m.split_arrow_width } else { arrow };
                        (m.small_icon + SMALL_FRAME + arrow_part).max(m.small_width)
                    } else {
                        let icon = if common::icon_key(item).is_some() {
                            m.medium_icon + ICON_EXTRA
                        } else {
                            0.0
                        };
                        let arrow_part = if split { m.split_arrow_width } else { arrow };
                        (text + icon + arrow_part + ATOM_FRAME).min(MAX_LABEL_W)
                    }
                }
            }
        })
        .collect()
}

/// The width of a \u{25be} arrow in the command bar (`arrow_atom(10.0, ...)`)
/// plus its spacing.
const ARROW_EXTRA: f32 = 20.0;

/// The icon's width allowance over the label on the same row (the
/// icon plus its spacing).
const ICON_EXTRA: f32 = 28.0;

/// The frame and atom spacing of a medium command-bar button (plus
/// the slack that keeps a text atom from shrinking into a wrap).
const ATOM_FRAME: f32 = 36.0;

/// The frame and atom spacing of a large button (the icon-over-label
/// stack), plus the same slack.
const LARGE_FRAME: f32 = 38.0;

/// The frame and atom spacing of a small (icon-only) button.
const SMALL_FRAME: f32 = 32.0;

/// The frame and atom spacing of a check box with its label, plus
/// slack.
const CHECK_FRAME: f32 = 18.0;

/// The space a combo takes around its label (padding and the arrow),
/// plus slack.
const COMBO_EXTRA: f32 = 60.0;

/// The widest a measured control gets: a malformed label (or a huge
/// theme font) cannot take the whole command bar (AGENTS.md section
/// 6: bounded allocations on input-derived values).
const MAX_LABEL_W: f32 = 400.0;

/// The width of `text` in the font the buttons draw with (the
/// theme's `ui` font once the theme applies), in logical pixels.
///
/// Measuring in egui's default proportional font under-measured the
/// theme's Selawik labels, so a combo label was cut short (task P7-17).
fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
        .rect
        .width()
}

/// The tab-set name of a contextual tab, small and in the set's text
/// colour, or `None` for a core tab.
fn tab_set_caption(ctx: &egui::Context, tab: &Tab) -> Option<egui::RichText> {
    tab.shown_in?;
    let name = tab.tab_set.clone().unwrap_or_default();
    let (_, _, text_token) =
        contextual_tokens(set_family(ctx, &name).as_deref().unwrap_or_default());
    Some(
        egui::RichText::new(name)
            .font(crate::theme::font_at(ctx, "caption"))
            .color(crate::theme::color_at(ctx, text_token)),
    )
}

/// Paints the selected tab's accent underline along the bottom of
/// `rect` (`tab.underline` thick, inset by `tab.padding_x` less a few
/// pixels so it spans the caption; docs/ui/design.md).
fn paint_tab_underline(ui: &Ui, rect: Rect) {
    let ctx = ui.ctx();
    let thickness = crate::theme::metric_at(ctx, "tab.underline", 2.0);
    let inset = (crate::theme::metric_at(ctx, "tab.padding_x", 12.0) - 4.0).max(0.0);
    let line = Rect::from_min_max(
        egui::pos2(rect.min.x + inset, rect.max.y - thickness),
        egui::pos2(rect.max.x - inset, rect.max.y),
    );
    ui.painter().rect_filled(
        line,
        crate::theme::metric_at(ctx, "radius.control", 0.0).min(thickness / 2.0),
        crate::theme::color_at(ctx, "accent"),
    );
}

/// Paints the contextual-tab look of one tab: the set's fill behind
/// the caption (the band is painted by the set caption block).
fn paint_contextual_tab(ui: &Ui, rect: Rect, tab: &Tab) {
    let set_name = tab.tab_set.clone().unwrap_or_default();
    let (_, fill_token, _) = contextual_tokens(
        set_family(ui.ctx(), &set_name)
            .as_deref()
            .unwrap_or_default(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, crate::theme::color_at(ui.ctx(), fill_token));
}

/// The colour family of one contextual tab set (`contextual_tab_sets`
/// in the theme; ADR 0064).
fn set_family(ctx: &egui::Context, set_name: &str) -> Option<String> {
    crate::theme::current(ctx).and_then(|theme| theme.contextual_tab_sets.get(set_name).cloned())
}

/// The three colour tokens of one contextual family: band, fill and
/// text (`contextual.<family>.*`). An unknown family draws as green.
fn contextual_tokens(family: &str) -> (&'static str, &'static str, &'static str) {
    match family {
        "orange" => (
            "contextual.orange.band",
            "contextual.orange.fill",
            "contextual.orange.text",
        ),
        _ => (
            "contextual.green.band",
            "contextual.green.fill",
            "contextual.green.text",
        ),
    }
}

#[cfg(test)]
mod tests;
