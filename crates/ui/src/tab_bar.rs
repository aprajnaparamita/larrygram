//! The command bar, the quick access toolbar, the backstage and the
//! context menus (ADR 0031), built from the ui-spec layouts.
//!
//! The command bar is data-driven: [`TabBar::show`] walks the layout tree
//! by placement path and renders one widget per placement, with the
//! ADR 0031 widget mapping per control type. Every actionable widget
//! produces an [`Intent`] (ADR 0028); widgets never execute commands
//! themselves. A control whose command is not implemented renders
//! disabled with a "Not yet implemented" tooltip; a control listed in
//! `ui-spec/out-of-scope.txt` renders disabled with an "out of scope"
//! tooltip.
//!
//! Owned by task P4-04.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Button, ComboBox, RichText, Ui};
use larrygram_commands::{AppCtx, CommandId, CommandStatus, Enabled, Invocation, Registry, Source};
use larrygram_ui_spec::{
    Control, ControlType, FluentId, Group, LayoutKind, LayoutSet, embedded::is_out_of_scope,
};

use crate::{Intent, UiError, icons};

/// Tooltip of a control whose command has no handler yet (ADR 0031).
const NOT_YET_IMPLEMENTED: &str = "Not yet implemented";

/// Tooltip of a control listed in `ui-spec/out-of-scope.txt` (ADR 0031).
const OUT_OF_SCOPE: &str = "Out of scope for this run";

/// The tooltip of a split button's dropdown arrow (ADR 0031). It
/// doubles as the arrow's accessible name: the arrow is a painted
/// mark (egui's fonts have no triangle glyph) and tooltip text is
/// ours.
const DROPDOWN_TIP: &str = "More items";

/// The label of the File button that opens the backstage (ADR 0031).
const FILE_BUTTON: &str = "File";

/// Placeholder icon size in the command bar, in points (ADR 0030).
const ICON_SIZE: f32 = 12.0;

/// Recursion cap for nested control stacks (AGENTS.md section 6).
const MAX_DEPTH: u32 = 8;

/// What one control does when it is activated (ADR 0031).
#[derive(Debug, Clone, PartialEq)]
enum Action {
    /// Dispatch this intent when the widget is clicked.
    Emit(Intent),
    /// Render the widget disabled with this tooltip.
    Disabled {
        /// The tooltip text (ours, never Microsoft's).
        reason: String,
    },
}

/// How the command bar resolves a control to its [`Action`].
///
/// [`TabBar::show`] resolves through the command registry (ADR 0028);
/// the tests drive the same widget code with their own mapping. The
/// registry seam is thin ([`action_for`]); everything else is shared.
type Resolve<'a> = &'a dyn Fn(&Control) -> Action;

/// The action of a control whose `command` link resolved to `id`
/// (ADR 0031 control-to-command rules).
///
/// An implemented and enabled command becomes an [`Intent`] with an
/// empty [`Invocation`] from [`Source::CommandBar`]. Everything else
/// renders disabled: the [`Enabled`] reason when the command is
/// implemented but disabled, else "Not yet implemented".
fn action_for_command(id: Option<CommandId>, status: CommandStatus, enabled: &Enabled) -> Action {
    if let Some(id) = id
        && status == CommandStatus::Implemented
        && enabled.is_enabled()
    {
        return Action::Emit(Intent::new(id, Invocation::empty(Source::CommandBar)));
    }
    let reason = if status == CommandStatus::Implemented {
        enabled
            .reason
            .clone()
            .unwrap_or_else(|| NOT_YET_IMPLEMENTED.to_owned())
    } else {
        NOT_YET_IMPLEMENTED.to_owned()
    };
    Action::Disabled { reason }
}

/// The action of one control through the command registry (ADR 0031).
///
/// A control with no `command` link, or a link that does not parse
/// into a registry id, is disabled like a stub. A control listed in
/// `ui-spec/out-of-scope.txt` is disabled with the out-of-scope
/// tooltip whatever its command says.
fn action_for(control: &Control, registry: &Registry, app: &AppCtx) -> Action {
    if is_out_of_scope(control.id.as_str()) {
        return Action::Disabled {
            reason: OUT_OF_SCOPE.to_owned(),
        };
    }
    let stub = Action::Disabled {
        reason: NOT_YET_IMPLEMENTED.to_owned(),
    };
    let Some(command) = &control.command else {
        return stub;
    };
    let Ok(id) = CommandId::try_from(command) else {
        return stub;
    };
    let status = registry.status(&id);
    let enabled = registry.enabled(&id, app);
    action_for_command(Some(id), status, &enabled)
}

/// One rendered placement: a control plus the controls nested in it.
///
/// A placement is identified by its path in the layout (ADR 0007
/// amendment, ADR 0031): the same Fluent id may occur several times,
/// and each occurrence is its own placement.
#[derive(Debug)]
struct Placement<'c> {
    /// The control itself.
    control: &'c Control,
    /// The controls whose `parent` is this control (first wins when
    /// several kept controls share one id).
    children: Vec<Placement<'c>>,
}

/// The placements of one container's control list (ADR 0031).
///
/// - Repeats inside one container with identical id, label **and**
///   order render once: the first occurrence wins, later ones are
///   dropped. Repeats that differ in any of the three are separate
///   placements and each renders.
/// - A control whose `parent` names a control of this container
///   renders nested in that control (a dropdown item for the
///   dropdown types, a stacked entry for the structural ones), never
///   twice.
/// - A control whose parent is outside this container renders as a
///   root. So does every control a recursion cap or a parent cycle
///   would otherwise hide: each kept control renders exactly once.
fn placements(controls: &[Control]) -> Vec<Placement<'_>> {
    let kept = kept(controls);
    let ids: BTreeSet<&str> = kept.iter().map(|control| control.id.as_str()).collect();
    let mut by_parent: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, control) in kept.iter().enumerate() {
        if let Some(parent) = &control.parent
            && ids.contains(parent.as_str())
        {
            by_parent.entry(parent.as_str()).or_default().push(index);
        }
    }
    let mut claimed: BTreeSet<usize> = BTreeSet::new();
    let mut out = Vec::new();
    for (index, control) in kept.iter().enumerate() {
        let nested = control
            .parent
            .as_ref()
            .is_some_and(|parent| ids.contains(parent.as_str()));
        if !nested && claimed.insert(index) {
            out.push(subtree(&kept, index, &by_parent, &mut claimed, 0));
        }
    }
    // Controls no root claimed (parent cycles, or deeper than the
    // recursion cap) render as extra roots.
    for index in 0..kept.len() {
        if claimed.insert(index) {
            out.push(subtree(&kept, index, &by_parent, &mut claimed, 0));
        }
    }
    out
}

/// The kept controls of one container: the first occurrence of every
/// (id, label, order) triple (ADR 0031 dedup, first wins).
fn kept(controls: &[Control]) -> Vec<&Control> {
    let mut seen: BTreeSet<(&str, &str, u32)> = BTreeSet::new();
    controls
        .iter()
        .filter(|control| seen.insert((control.id.as_str(), control.label.as_str(), control.order)))
        .collect()
}

/// Builds the subtree under `kept[index]`, claiming each control at
/// most once.
///
/// `depth` bounds the recursion (AGENTS.md section 6). Beyond
/// [`MAX_DEPTH`] the descendants stay unclaimed; [`placements`] then
/// renders them as roots, so no placement is dropped and no input can
/// make this recurse without bound.
fn subtree<'c>(
    kept: &[&'c Control],
    index: usize,
    by_parent: &BTreeMap<&str, Vec<usize>>,
    claimed: &mut BTreeSet<usize>,
    depth: u32,
) -> Placement<'c> {
    claimed.insert(index);
    let control = kept[index];
    let children = if depth < MAX_DEPTH {
        let unclaimed: Vec<usize> = by_parent
            .get(control.id.as_str())
            .into_iter()
            .flatten()
            .copied()
            .filter(|child| !claimed.contains(child))
            .collect();
        unclaimed
            .into_iter()
            .map(|child| subtree(kept, child, by_parent, claimed, depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    Placement { control, children }
}

/// How the controls of one container flow (ADR 0031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Controls flow in rows (command bar groups).
    Rows,
    /// One horizontal row (quick access toolbar).
    Row,
    /// Controls stack vertically (backstage, context menus).
    Stack,
}

/// Which affordance opens a dropdown (ADR 0031).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropKind {
    /// A labelled button (gallery, dropDown, menu).
    Button,
    /// The small dropdown arrow button of a split button.
    Arrow,
}

/// The command-bar widget state (ADR 0031).
pub struct TabBar {
    kind: LayoutKind,
    active_tab: usize,
    active_backstage_tab: usize,
    backstage_open: bool,
    context_menu: Option<FluentId>,
    /// Display-only toggle state for `toggleButton` and `checkBox`
    /// controls (the command state lives in `AppCtx`, ADR 0029; this
    /// only keeps the widget from flickering while nothing owns the
    /// real state).
    toggles: BTreeSet<FluentId>,
}

impl TabBar {
    /// A tab bar for one layout kind, first tab active, backstage closed.
    pub fn new(kind: LayoutKind) -> Self {
        Self {
            kind,
            active_tab: 0,
            active_backstage_tab: 0,
            backstage_open: false,
            context_menu: None,
            toggles: BTreeSet::new(),
        }
    }

    /// The layout kind this tab bar renders.
    pub fn kind(&self) -> LayoutKind {
        self.kind
    }

    /// The index of the active tab in the layout's `tabs` list.
    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    /// Whether the backstage (File) view is open.
    pub fn backstage_open(&self) -> bool {
        self.backstage_open
    }

    /// The index of the active backstage tab.
    pub fn active_backstage_tab(&self) -> usize {
        self.active_backstage_tab
    }

    /// The context menu [`TabBar::show_context_menu`] draws, if any.
    pub fn context_menu(&self) -> Option<&FluentId> {
        self.context_menu.as_ref()
    }

    /// Switches the layout kind, resetting to the first tab (ADR 0032).
    pub fn set_kind(&mut self, kind: LayoutKind) {
        self.kind = kind;
        self.active_tab = 0;
    }

    /// Activates the tab at `index` (clamped to the tab list on the
    /// next frame; out-of-range values simply show no groups).
    pub fn set_active_tab(&mut self, index: usize) {
        self.active_tab = index;
    }

    /// Opens or closes the backstage (File) view.
    pub fn set_backstage_open(&mut self, open: bool) {
        self.backstage_open = open;
    }

    /// Activates the backstage entry at `index` (out-of-range values
    /// simply show no controls).
    pub fn set_active_backstage_tab(&mut self, index: usize) {
        self.active_backstage_tab = index;
    }

    /// Selects the context menu drawn by [`TabBar::show_context_menu`]
    /// (`None` draws nothing, e.g. when no canvas was right-clicked).
    pub fn set_context_menu(&mut self, id: Option<FluentId>) {
        self.context_menu = id;
    }

    /// Draws the tab strip and the active tab's groups (ADR 0031).
    ///
    /// Controls without an implemented command render disabled with a
    /// "Not yet implemented" tooltip. Clicking a control produces its
    /// [`Intent`]; the File button toggles the backstage and clicking
    /// a tab activates it (no intents).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        registry: &Registry,
        app: &AppCtx,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_with(ui, set, &|control| action_for(control, registry, app))
    }

    /// Draws the quick access toolbar (ADR 0031).
    ///
    /// A small row of the layout's QAT placements, with the same
    /// control-to-command rules as the command bar.
    pub fn show_quick_access(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        registry: &Registry,
        app: &AppCtx,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_quick_access_with(ui, set, &|control| action_for(control, registry, app))
    }

    /// Draws the backstage tabs (File view) as a full-window stack
    /// (ADR 0031).
    ///
    /// Nothing is drawn while the backstage is closed. Otherwise the
    /// layout's backstage tabs render as a vertical list of entries
    /// and the active entry's controls stack beside it.
    pub fn show_backstage(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        registry: &Registry,
        app: &AppCtx,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_backstage_with(ui, set, &|control| action_for(control, registry, app))
    }

    /// Draws one context menu from the layout (right-click, ADR 0031).
    ///
    /// Which menu is drawn is state: [`TabBar::set_context_menu`] names
    /// it (the canvas sets it on right-click, ADR 0031). With no menu
    /// selected nothing is drawn.
    pub fn show_context_menu(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        registry: &Registry,
        app: &AppCtx,
    ) -> Result<Vec<Intent>, UiError> {
        self.show_context_menu_with(ui, set, &|control| action_for(control, registry, app))
    }

    /// [`TabBar::show`] with an injected command-state resolver.
    fn show_with(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        let layout = &set.layout;
        let mut out = Vec::new();
        ui.horizontal(|ui| {
            if ui.button(FILE_BUTTON).clicked() {
                self.backstage_open = !self.backstage_open;
            }
            for (index, tab) in layout.tabs.iter().enumerate() {
                let response = ui.selectable_label(self.active_tab == index, tab.label.as_str());
                if response.clicked() {
                    self.active_tab = index;
                }
            }
        });
        if let Some(tab) = layout.tabs.get(self.active_tab) {
            ui.horizontal_wrapped(|ui| -> Result<(), UiError> {
                for group in &tab.groups {
                    self.show_group(ui, group, resolve, &mut out)?;
                }
                Ok(())
            })
            .inner?;
        }
        Ok(out)
    }

    /// [`TabBar::show_quick_access`] with an injected resolver.
    fn show_quick_access_with(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        let mut out = Vec::new();
        self.show_flow(
            ui,
            &set.layout.quick_access_toolbar.controls,
            Flow::Row,
            resolve,
            &mut out,
            0,
        )?;
        Ok(out)
    }

    /// [`TabBar::show_backstage`] with an injected resolver.
    fn show_backstage_with(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        if !self.backstage_open {
            return Ok(Vec::new());
        }
        let tabs = &set.layout.backstage.tabs;
        let mut out = Vec::new();
        ui.horizontal_top(|ui| {
            // The File entries, one per backstage tab placement.
            ui.vertical(|ui| {
                for (index, tab) in tabs.iter().enumerate() {
                    let response =
                        ui.selectable_label(self.active_backstage_tab == index, tab.label.as_str());
                    if response.clicked() {
                        self.active_backstage_tab = index;
                    }
                }
            });
            // The active entry's controls, as a vertical stack.
            match tabs.get(self.active_backstage_tab) {
                Some(tab) => self.show_flow(ui, &tab.controls, Flow::Stack, resolve, &mut out, 0),
                None => Ok(()),
            }
        })
        .inner?;
        Ok(out)
    }

    /// [`TabBar::show_context_menu`] with an injected resolver.
    fn show_context_menu_with(
        &mut self,
        ui: &mut Ui,
        set: &LayoutSet,
        resolve: Resolve<'_>,
    ) -> Result<Vec<Intent>, UiError> {
        let Some(id) = self.context_menu.clone() else {
            return Ok(Vec::new());
        };
        let Some(menu) = set.layout.context_menus.iter().find(|menu| menu.id == id) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        self.show_flow(ui, &menu.controls, Flow::Stack, resolve, &mut out, 0)?;
        Ok(out)
    }

    /// Renders one command-bar group: a titled box with the controls in
    /// rows and a bottom caption holding the mark, the label and the
    /// dialog box launcher (ADR 0031).
    fn show_group(
        &mut self,
        ui: &mut Ui,
        group: &Group,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
    ) -> Result<(), UiError> {
        let placements = placements(&group.controls);
        egui::Frame::group(ui.style())
            .show(ui, |ui| -> Result<(), UiError> {
                ui.vertical(|ui| -> Result<(), UiError> {
                    // The controls flow in rows (ADR 0031); the
                    // dialog box launchers belong to the caption.
                    let flow: Vec<&Placement<'_>> = placements
                        .iter()
                        .filter(|placement| {
                            placement.control.r#type != ControlType::ButtonDialogBoxLauncher
                        })
                        .collect();
                    self.show_in_flow(ui, &flow, Flow::Rows, resolve, out, 0)?;
                    // The bottom caption.
                    ui.horizontal(|ui| -> Result<(), UiError> {
                        icons::placeholder(ui, group.id.as_str(), ICON_SIZE)?;
                        ui.label(group.label.as_str());
                        for placement in &placements {
                            if placement.control.r#type != ControlType::ButtonDialogBoxLauncher {
                                continue;
                            }
                            // A small corner button in the group
                            // caption (ADR 0031).
                            let action = resolve(placement.control);
                            let response = ui.add_enabled(
                                matches!(action, Action::Emit(_)),
                                egui::Button::new((
                                    icons::icon(placement.control.id.as_str(), ICON_SIZE),
                                    placement.control.label.as_str(),
                                ))
                                .small(),
                            );
                            finish(response, &action, out);
                        }
                        Ok(())
                    })
                    .inner
                })
                .inner
            })
            .inner
    }

    /// Renders one container's controls in the given flow.
    fn show_flow(
        &mut self,
        ui: &mut Ui,
        controls: &[Control],
        flow: Flow,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
        depth: u32,
    ) -> Result<(), UiError> {
        let placements = placements(controls);
        let all: Vec<&Placement<'_>> = placements.iter().collect();
        self.show_in_flow(ui, &all, flow, resolve, out, depth)
    }

    /// Renders placements in the given flow, each in its own id scope
    /// so repeated labels in one container do not clash widget ids.
    fn show_in_flow(
        &mut self,
        ui: &mut Ui,
        placements: &[&Placement<'_>],
        flow: Flow,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
        depth: u32,
    ) -> Result<(), UiError> {
        let inner = |ui: &mut Ui| -> Result<(), UiError> {
            for (index, placement) in placements.iter().enumerate() {
                ui.push_id(index, |ui| {
                    self.show_placement(ui, placement, resolve, out, depth)
                })
                .inner?;
            }
            Ok(())
        };
        match flow {
            Flow::Rows => ui.horizontal_wrapped(inner).inner,
            Flow::Row => ui.horizontal(inner).inner,
            Flow::Stack => ui.vertical(inner).inner,
        }
    }

    /// Renders one placement and the controls nested in it (ADR 0031).
    ///
    /// `depth` bounds the nesting (AGENTS.md section 6): deeper
    /// placements render as plain labels.
    fn show_placement(
        &mut self,
        ui: &mut Ui,
        placement: &Placement<'_>,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
        depth: u32,
    ) -> Result<(), UiError> {
        let control = placement.control;
        if depth >= MAX_DEPTH {
            ui.label(control.label.as_str());
            return Ok(());
        }
        let action = resolve(control);
        let enabled = matches!(action, Action::Emit(_));
        match control.r#type {
            ControlType::Button | ControlType::ButtonDialogBoxLauncher => {
                let response = ui.add_enabled(enabled, control_button(control));
                finish(response, &action, out);
            }
            ControlType::ToggleButton => {
                let selected = self.toggles.contains(&control.id);
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        ui.selectable_label(
                            selected,
                            (
                                icons::icon(control.id.as_str(), ICON_SIZE),
                                control.label.as_str(),
                            ),
                        )
                    })
                    .inner;
                if response.clicked() {
                    self.toggle(control.id.clone());
                }
                finish(response, &action, out);
            }
            ControlType::CheckBox => {
                let mut checked = self.toggles.contains(&control.id);
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        ui.checkbox(&mut checked, control.label.as_str())
                    })
                    .inner;
                if response.clicked() {
                    if checked {
                        self.toggles.insert(control.id.clone());
                    } else {
                        self.toggles.remove(&control.id);
                    }
                }
                finish(response, &action, out);
            }
            ControlType::SplitButton => {
                // A button plus a small dropdown arrow button in one
                // row (ADR 0031): the button carries the command, the
                // arrow opens the items.
                ui.horizontal(|ui| -> Result<(), UiError> {
                    let response = ui.add_enabled(enabled, control_button(control));
                    finish(response, &action, out);
                    if !placement.children.is_empty() {
                        self.show_dropdown(
                            ui,
                            DropKind::Arrow,
                            placement,
                            resolve,
                            out,
                            depth + 1,
                        )?;
                    }
                    Ok(())
                })
                .inner?;
            }
            ControlType::Gallery | ControlType::Menu | ControlType::DropDown => {
                // One button that opens a dropdown listing the item
                // labels (ADR 0031). With items it is the dropdown
                // affordance and the items carry the commands; without
                // items it is a plain button under the command rules.
                if placement.children.is_empty() {
                    let response = ui.add_enabled(enabled, control_button(control));
                    finish(response, &action, out);
                } else {
                    self.show_dropdown(ui, DropKind::Button, placement, resolve, out, depth + 1)?;
                }
            }
            ControlType::ComboBox => {
                let shown = ComboBox::from_id_salt(control.id.as_str())
                    .selected_text(control.label.as_str())
                    .show_ui(ui, |ui| -> Result<(), UiError> {
                        for child in &placement.children {
                            self.show_placement(ui, child, resolve, out, depth + 1)?;
                        }
                        Ok(())
                    });
                // A combo box with no label atom has no accessible
                // name; egui documents `on_hover_text` as the fix and
                // the tooltip text is ours (ADR 0031).
                shown.response.on_hover_text(control.label.as_str());
                // `None` while the dropdown is closed.
                if let Some(result) = shown.inner {
                    result?;
                }
            }
            ControlType::LabelControl | ControlType::Control | ControlType::None => {
                // Non-actionable labels (ADR 0031).
                ui.label(control.label.as_str());
            }
            ControlType::Group
            | ControlType::Task
            | ControlType::TaskFormGroup
            | ControlType::Category
            | ControlType::Tab
            | ControlType::TabSet
            | ControlType::ContextMenu => {
                // Backstage section headings and structural groups
                // (ADR 0031): a heading plus the nested controls.
                let caption_last = control.r#type == ControlType::Group;
                ui.group(|ui| -> Result<(), UiError> {
                    if !caption_last {
                        ui.label(RichText::new(control.label.as_str()).strong());
                    }
                    for child in &placement.children {
                        self.show_placement(ui, child, resolve, out, depth + 1)?;
                    }
                    if caption_last {
                        ui.label(control.label.as_str());
                    }
                    Ok(())
                })
                .inner?;
            }
        }
        // Controls whose kind nests nothing of its own (plain
        // buttons, toggles, labels) still render their nested
        // controls, so that no placement is dropped (ADR 0031).
        if !kind_handles_children(control.r#type) {
            for child in &placement.children {
                self.show_placement(ui, child, resolve, out, depth + 1)?;
            }
        }
        Ok(())
    }

    /// Renders one dropdown affordance: a button that opens a menu
    /// listing the item labels (ADR 0031).
    fn show_dropdown(
        &mut self,
        ui: &mut Ui,
        kind: DropKind,
        placement: &Placement<'_>,
        resolve: Resolve<'_>,
        out: &mut Vec<Intent>,
        depth: u32,
    ) -> Result<(), UiError> {
        let control = placement.control;
        let button = match kind {
            DropKind::Button => egui::Button::new((
                icons::icon(control.id.as_str(), ICON_SIZE),
                control.label.as_str(),
            )),
            DropKind::Arrow => egui::Button::new(icons::dropdown_arrow(ICON_SIZE)).small(),
        };
        let (response, popup) =
            egui::menu::MenuButton::from_button(button).ui(ui, |ui| -> Result<(), UiError> {
                for child in &placement.children {
                    self.show_placement(ui, child, resolve, out, depth)?;
                }
                Ok(())
            });
        if kind == DropKind::Arrow {
            // The arrow is a painted mark with no label of its own;
            // our tooltip doubles as its accessible name (ADR 0031).
            response.on_hover_text(DROPDOWN_TIP);
        }
        if let Some(popup) = popup {
            popup.inner?;
        }
        Ok(())
    }

    /// Records the display-only toggle of one control.
    fn toggle(&mut self, id: FluentId) {
        if !self.toggles.remove(&id) {
            self.toggles.insert(id);
        }
    }
}

/// The icon-plus-label button of one control (ADR 0030, ADR 0031).
fn control_button(control: &Control) -> Button<'_> {
    Button::new((
        icons::icon(control.id.as_str(), ICON_SIZE),
        control.label.as_str(),
    ))
}

/// True for control kinds that render their nested controls
/// themselves: dropdown items (one-button dropdowns, split arrows,
/// combo boxes) and the stack of a structural section (ADR 0031).
fn kind_handles_children(r#type: ControlType) -> bool {
    matches!(
        r#type,
        ControlType::SplitButton
            | ControlType::Gallery
            | ControlType::Menu
            | ControlType::DropDown
            | ControlType::ComboBox
            | ControlType::Group
            | ControlType::Task
            | ControlType::TaskFormGroup
            | ControlType::Category
            | ControlType::Tab
            | ControlType::TabSet
            | ControlType::ContextMenu
    )
}

/// Finishes one widget: dispatch its intent on click, or grey it out
/// with its tooltip (ADR 0031).
///
/// `on_hover_text` records the tooltip as the widget's accessible
/// description; `on_disabled_hover_text` shows it while the widget is
/// disabled.
fn finish(response: egui::Response, action: &Action, out: &mut Vec<Intent>) {
    match action {
        Action::Emit(intent) => {
            if response.clicked() {
                out.push(intent.clone());
            }
        }
        Action::Disabled { reason } => {
            response
                .on_hover_text(reason.as_str())
                .on_disabled_hover_text(reason.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;
    use egui::accesskit::Role;
    use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
    use larrygram_commands::Settings;
    use larrygram_ui_spec::{
        Backstage, BackstageTab, ContextMenu, LayoutSets, Loader, QuickAccessToolbar, Tab,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::OnceLock;

    // --- fixtures ---------------------------------------------------

    /// The committed layouts, parsed once per test binary.
    fn sets() -> &'static LayoutSets {
        static SETS: OnceLock<LayoutSets> = OnceLock::new();
        SETS.get_or_init(|| Loader::from_embedded().expect("the embedded files load"))
    }

    /// A test control.
    fn control(id: &str, label: &str, r#type: ControlType, command: Option<&str>) -> Control {
        Control {
            id: FluentId::new(id),
            r#type,
            label: label.to_owned(),
            order: 0,
            parent: None,
            command: command.map(larrygram_ui_spec::CommandId::new),
            aliases: Vec::new(),
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
        }
    }

    /// A test control nested in `parent`, with a sibling order.
    fn child(
        id: &str,
        label: &str,
        r#type: ControlType,
        command: Option<&str>,
        parent: &str,
        order: u32,
    ) -> Control {
        Control {
            parent: Some(FluentId::new(parent)),
            order,
            ..control(id, label, r#type, command)
        }
    }

    /// A one-tab layout with the given group controls.
    fn set_with(controls: Vec<Control>) -> LayoutSet {
        let mut layout = larrygram_ui_spec::Layout::default();
        layout.tabs.push(Tab {
            id: FluentId::new("TabHome"),
            label: "TabHome".to_owned(),
            order: 0,
            tab_set: None,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            groups: vec![Group {
                id: FluentId::new("GroupClipboard"),
                label: "GroupClipboard".to_owned(),
                order: 0,
                bounds: None,
                icon_ref: None,
                keytip: None,
                size_variant: None,
                visual_notes: None,
                controls,
            }],
        });
        LayoutSet {
            layout,
            ..LayoutSet::default()
        }
    }

    /// A quick intent for the tests.
    fn intent(id: &'static str) -> Intent {
        Intent::new(
            CommandId::parse(id).expect("a valid command id"),
            Invocation::empty(Source::CommandBar),
        )
    }

    // --- the driving rig -------------------------------------------

    /// Which of the four `show` methods a [`Rig`] runs each frame.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Surface {
        /// [`TabBar::show_with`].
        CommandBar,
        /// [`TabBar::show_with`] and [`TabBar::show_quick_access_with`].
        CommandBarAndQuickAccess,
        /// [`TabBar::show_quick_access_with`].
        QuickAccess,
        /// [`TabBar::show_backstage_with`].
        Backstage,
        /// [`TabBar::show_context_menu_with`].
        ContextMenu,
        /// [`TabBar::show`], through the real command registry.
        Registry,
    }

    /// A command bar driven headlessly by `egui_kittest`.
    ///
    /// The harness closure keeps running between the test's
    /// assertions, so the command bar, the intents and the resolved action
    /// are shared through `Rc<RefCell<..>>` and the test drives them
    /// between frames.
    struct Rig {
        tab_bar: Rc<RefCell<TabBar>>,
        intents: Rc<RefCell<Vec<Intent>>>,
        action: Rc<RefCell<Action>>,
        harness: Harness<'static>,
    }

    impl Rig {
        /// Builds a rig for one layout set and surface. `set: None`
        /// follows the command bar's layout kind (the ADR 0032 switch).
        fn build(kind: LayoutKind, set: Option<LayoutSet>, surface: Surface) -> Self {
            let tab_bar = Rc::new(RefCell::new(TabBar::new(kind)));
            let intents = Rc::new(RefCell::new(Vec::new()));
            let action = Rc::new(RefCell::new(Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned(),
            }));
            let fixed = set.map(Rc::new);
            // Wide enough that every placement of the committed
            // command bars sits inside the screen rect (clicks outside it
            // never reach a widget).
            let harness = Harness::builder()
                .with_size(egui::vec2(12000.0, 4000.0))
                .build_ui({
                    let tab_bar = Rc::clone(&tab_bar);
                    let intents = Rc::clone(&intents);
                    let action = Rc::clone(&action);
                    let registry = Registry::new();
                    let app = AppCtx::new(Settings::default());
                    move |ui| {
                        let action = action.borrow().clone();
                        let resolve = move |_: &Control| action.clone();
                        let dynamic;
                        let set: &LayoutSet = match fixed {
                            Some(ref set) => set.as_ref(),
                            None => {
                                dynamic = sets().get(tab_bar.borrow().kind());
                                dynamic
                            }
                        };
                        let mut tab_bar = tab_bar.borrow_mut();
                        let produced = match surface {
                            Surface::CommandBar => tab_bar.show_with(ui, set, &resolve),
                            Surface::CommandBarAndQuickAccess => {
                                let mut intents = tab_bar
                                    .show_with(ui, set, &resolve)
                                    .expect("the command bar renders");
                                intents.extend(
                                    tab_bar
                                        .show_quick_access_with(ui, set, &resolve)
                                        .expect("the quick access renders"),
                                );
                                Ok(intents)
                            }
                            Surface::QuickAccess => {
                                tab_bar.show_quick_access_with(ui, set, &resolve)
                            }
                            Surface::Backstage => tab_bar.show_backstage_with(ui, set, &resolve),
                            Surface::ContextMenu => {
                                tab_bar.show_context_menu_with(ui, set, &resolve)
                            }
                            Surface::Registry => tab_bar.show(ui, set, &registry, &app),
                        }
                        .expect("the command bar renders");
                        // Accumulate: one `run` may take several
                        // frames and only the first sees the click.
                        intents.borrow_mut().extend(produced);
                    }
                });
            let mut rig = Self {
                tab_bar,
                intents,
                action,
                harness,
            };
            rig.run();
            rig
        }

        /// A rig for one layout set, drawing the command bar.
        fn new(kind: LayoutKind, set: LayoutSet, surface: Surface) -> Self {
            Self::build(kind, Some(set), surface)
        }

        /// A rig for one committed layout, drawing the command bar.
        fn committed(kind: LayoutKind) -> Self {
            Self::new(kind, sets().get(kind).clone(), Surface::CommandBar)
        }

        /// A rig over the committed layouts that follows the command bar's
        /// layout kind: the ADR 0032 switch.
        fn switching(kind: LayoutKind) -> Self {
            Self::build(kind, None, Surface::CommandBar)
        }

        /// Runs one frame.
        fn run(&mut self) {
            self.harness.run();
        }

        /// The action the resolver reports for every control.
        fn set_action(&self, action: Action) {
            *self.action.borrow_mut() = action;
        }

        /// The intents the widgets produced since the last call.
        fn take_intents(&self) -> Vec<Intent> {
            std::mem::take(&mut *self.intents.borrow_mut())
        }

        /// Mutates the command bar state between frames (layout switch,
        /// backstage, ...).
        fn tab_bar(&self) -> std::cell::RefMut<'_, TabBar> {
            self.tab_bar.borrow_mut()
        }

        /// Clicks the first widget with this role and label, then
        /// runs a frame.
        fn click(&mut self, role: Role, label: &str) {
            {
                let root = self.harness.root();
                let node = root
                    .query_all_by_role_and_label(role, label)
                    .next()
                    .unwrap_or_else(|| panic!("no {role:?} named {label:?}"));
                node.click();
            }
            self.run();
        }

        /// Every rendered widget of the current frame, in tree order.
        fn widgets(&self) -> Vec<Widget> {
            widgets(&self.harness)
        }

        /// The widget names of the current frame, as a multiset.
        fn names(&self) -> BTreeMap<String, usize> {
            names(&self.harness)
        }
    }

    /// The roles that count as one rendered widget (ADR 0031).
    fn is_widget_role(role: Role) -> bool {
        matches!(
            role,
            Role::Button
                | Role::CheckBox
                | Role::ComboBox
                | Role::Label
                | Role::MenuItem
                | Role::ListBoxOption
        )
    }

    /// One rendered widget node.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Widget {
        role: Role,
        name: String,
        disabled: bool,
        description: Option<String>,
    }

    /// Every rendered widget of a harness frame, in tree order.
    fn widgets(harness: &Harness<'_>) -> Vec<Widget> {
        let root = harness.root();
        let mut out = Vec::new();
        for node in root.children_recursive() {
            let data = node.accesskit_node().data();
            if !is_widget_role(data.role()) {
                continue;
            }
            let label = data.label().unwrap_or_default();
            let name = if label.is_empty() {
                data.value().unwrap_or_default()
            } else {
                label
            };
            if name.is_empty() {
                continue;
            }
            out.push(Widget {
                role: data.role(),
                name: name.to_owned(),
                disabled: data.is_disabled(),
                description: data.description().map(str::to_owned),
            });
        }
        out
    }

    /// The widget names of a harness frame, as a multiset.
    fn names(harness: &Harness<'_>) -> BTreeMap<String, usize> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for widget in widgets(harness) {
            *counts.entry(widget.name).or_default() += 1;
        }
        counts
    }

    /// Multiset difference `after - before`.
    fn added_names(
        before: &BTreeMap<String, usize>,
        after: &BTreeMap<String, usize>,
    ) -> Vec<String> {
        let mut added = Vec::new();
        for (name, count) in after {
            let seen = before.get(name).copied().unwrap_or(0);
            for _ in seen..*count {
                added.push(name.clone());
            }
        }
        added.sort();
        added
    }

    /// Clicks the dropdown affordance of one control and runs a
    /// frame: the labelled button of a gallery/dropDown/menu, or the
    /// arrow of a split button (named by its tooltip, right of its
    /// primary button).
    fn click_dropdown(rig: &mut Rig, control: &Control) {
        if control.r#type == ControlType::SplitButton {
            let primary = {
                let root = rig.harness.root();
                root.query_all_by_role_and_label(Role::Button, &control.label)
                    .next()
                    .unwrap_or_else(|| panic!("no button named {:?}", control.label))
                    .rect()
            };
            let target = egui::pos2(primary.right() + 2.0, primary.center().y);
            {
                let root = rig.harness.root();
                let arrow = root
                    .query_all_by_role_and_label(Role::Button, DROPDOWN_TIP)
                    .min_by(|a, b| {
                        let da = a.rect().left_top().distance(target);
                        let db = b.rect().left_top().distance(target);
                        da.total_cmp(&db)
                    })
                    .expect("a split arrow renders");
                arrow.click();
            }
            rig.run();
        } else {
            rig.click(Role::Button, &control.label);
        }
    }

    // --- placement accounting (test side, ADR 0031 rules) -----------

    /// The kept controls of one container: the first occurrence of
    /// every (id, label, order) triple.
    fn expected_kept(controls: &[Control]) -> Vec<&Control> {
        let mut seen: BTreeSet<(&str, &str, u32)> = BTreeSet::new();
        controls
            .iter()
            .filter(|c| seen.insert((c.id.as_str(), c.label.as_str(), c.order)))
            .collect()
    }

    /// True for controls whose nested controls are dropdown items.
    fn is_dropdown(r#type: ControlType) -> bool {
        matches!(
            r#type,
            ControlType::Gallery
                | ControlType::Menu
                | ControlType::DropDown
                | ControlType::ComboBox
                | ControlType::SplitButton
        )
    }

    /// The nested controls of `id`, in list order.
    fn children_of<'c>(kept: &[&'c Control], id: &str) -> Vec<&'c Control> {
        kept.iter()
            .copied()
            .filter(|c| {
                c.parent
                    .as_ref()
                    .is_some_and(|parent| parent.as_str() == id)
            })
            .collect()
    }

    /// True when another control of the same container is this one's
    /// parent (so the control renders nested, not as a root).
    fn is_nested(kept: &[&Control], control: &Control) -> bool {
        control.parent.as_ref().is_some_and(|parent| {
            kept.iter()
                .any(|other| other.id.as_str() == parent.as_str())
        })
    }

    /// The widget names one control renders by itself: a split button
    /// with items is a button plus a dropdown arrow (ADR 0031).
    fn widget_names(control: &Control, children: usize) -> Vec<String> {
        if control.r#type == ControlType::SplitButton && children > 0 {
            vec![control.label.clone(), DROPDOWN_TIP.to_owned()]
        } else {
            vec![control.label.clone()]
        }
    }

    /// The widget names one control list renders: the roots and the
    /// controls nested in them. With `open`, the items of dropdown
    /// controls count too.
    ///
    /// Written out from the ADR 0031 rules, independently of the
    /// renderer.
    fn expected_names(controls: &[Control], open: bool) -> Vec<String> {
        let kept = expected_kept(controls);
        let roots: Vec<&Control> = kept
            .iter()
            .copied()
            .filter(|c| !is_nested(&kept, c))
            .collect();
        let mut out = Vec::new();
        let mut seen: BTreeSet<(&str, &str, u32)> = BTreeSet::new();
        expected_walk(&kept, &roots, &mut out, open, &mut seen, true);
        // Controls in a parent cycle render as roots too.
        let leftover: Vec<&Control> = kept
            .iter()
            .copied()
            .filter(|c| !seen.contains(&(c.id.as_str(), c.label.as_str(), c.order)))
            .collect();
        expected_walk(&kept, &leftover, &mut out, open, &mut seen, true);
        out
    }

    /// Walks `items`, each kept control counted once.
    ///
    /// `visible` is false inside a closed dropdown: those controls are
    /// claimed (they are not leftovers) but render nothing until the
    /// dropdown opens.
    fn expected_walk<'c>(
        kept: &[&'c Control],
        items: &[&'c Control],
        out: &mut Vec<String>,
        open: bool,
        seen: &mut BTreeSet<(&'c str, &'c str, u32)>,
        visible: bool,
    ) {
        for control in items {
            if !seen.insert((control.id.as_str(), control.label.as_str(), control.order)) {
                continue;
            }
            let children = children_of(kept, control.id.as_str());
            if visible {
                out.extend(widget_names(control, children.len()));
            }
            // A closed dropdown hides its items; the nested controls of
            // every other control render with it.
            let kids_visible = visible && (open || !is_dropdown(control.r#type));
            expected_walk(kept, &children, out, open, seen, kids_visible);
        }
    }

    /// A name multiset of the given names.
    fn counts_of(names: &[String]) -> BTreeMap<String, usize> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for name in names {
            *counts.entry(name.clone()).or_default() += 1;
        }
        counts
    }

    /// The counts pinned per layout (ADR 0037: a change that adds a
    /// control shows up as a snapshot diff).
    fn counts_report(kind: LayoutKind, set: &LayoutSet) -> String {
        fn split(controls: &[Control]) -> (usize, usize) {
            let kept = expected_kept(controls);
            let roots = kept.iter().filter(|c| !is_nested(&kept, c)).count();
            (roots, kept.len() - roots)
        }
        let layout = &set.layout;
        let line = |label: &str, value: usize| format!("{label}: {value}");
        let mut tab_bar_roots = 0;
        let mut tab_bar_items = 0;
        let mut groups = 0;
        for tab in &layout.tabs {
            groups += tab.groups.len();
            for group in &tab.groups {
                let (roots, items) = split(&group.controls);
                tab_bar_roots += roots;
                tab_bar_items += items;
            }
        }
        let mut backstage_roots = 0;
        let mut backstage_items = 0;
        for tab in &layout.backstage.tabs {
            let (roots, items) = split(&tab.controls);
            backstage_roots += roots;
            backstage_items += items;
        }
        let mut menu_roots = 0;
        let mut menu_items = 0;
        for menu in &layout.context_menus {
            let (roots, items) = split(&menu.controls);
            menu_roots += roots;
            menu_items += items;
        }
        let (qat_roots, qat_items) = split(&layout.quick_access_toolbar.controls);
        [
            format!("layout: {}", kind_label(kind)),
            line("command bar tabs", layout.tabs.len()),
            line("command bar groups", groups),
            line("command bar controls", tab_bar_roots),
            line("command bar nested controls", tab_bar_items),
            line("backstage tabs", layout.backstage.tabs.len()),
            line("backstage controls", backstage_roots),
            line("backstage nested controls", backstage_items),
            line("context menus", layout.context_menus.len()),
            line("context menu controls", menu_roots),
            line("context menu nested controls", menu_items),
            line("quick access controls", qat_roots),
            line("quick access nested controls", qat_items),
        ]
        .join("\n")
    }

    /// The snapshot name of one layout kind (its layout id).
    fn kind_label(kind: LayoutKind) -> &'static str {
        kind.id()
    }

    // --- state ------------------------------------------------------

    #[test]
    fn tab_bar_state_transitions() {
        let mut tab_bar = TabBar::new(LayoutKind::Panel2016);
        assert_eq!(tab_bar.kind(), LayoutKind::Panel2016);
        assert_eq!(tab_bar.active_tab(), 0);
        assert_eq!(tab_bar.active_backstage_tab(), 0);
        assert!(!tab_bar.backstage_open());
        assert_eq!(tab_bar.context_menu(), None);

        tab_bar.set_kind(LayoutKind::Panel365);
        assert_eq!(tab_bar.kind(), LayoutKind::Panel365);
        assert_eq!(tab_bar.active_tab(), 0);

        tab_bar.set_active_tab(3);
        assert_eq!(tab_bar.active_tab(), 3);
        tab_bar.set_backstage_open(true);
        assert!(tab_bar.backstage_open());
        tab_bar.set_active_backstage_tab(2);
        assert_eq!(tab_bar.active_backstage_tab(), 2);
        let id = FluentId::new("ShapeShortcutMenu");
        tab_bar.set_context_menu(Some(id.clone()));
        assert_eq!(tab_bar.context_menu(), Some(&id));
    }

    #[test]
    fn toggles_flip_and_return_to_false() {
        let mut tab_bar = TabBar::new(LayoutKind::Panel2016);
        let id = FluentId::new("SnapToGrid");
        tab_bar.toggle(id.clone());
        assert!(tab_bar.toggles.contains(&id));
        tab_bar.toggle(id.clone());
        assert!(!tab_bar.toggles.contains(&id));
    }

    // --- control-to-command rules ----------------------------------

    #[test]
    fn an_implemented_enabled_command_produces_its_intent() {
        let id = CommandId::parse("file.save").expect("id");
        let action = action_for_command(Some(id), CommandStatus::Implemented, &Enabled::yes());
        let Action::Emit(got) = action else {
            panic!("expected an intent, got {action:?}");
        };
        assert_eq!(got.command, id);
        assert_eq!(got.invocation.source, Source::CommandBar);
        assert!(got.invocation.args.is_empty());
    }

    #[test]
    fn a_stub_or_missing_command_is_not_yet_implemented() {
        let id = Some(CommandId::parse("file.save").expect("id"));
        assert_eq!(
            action_for_command(id, CommandStatus::Stub, &Enabled::yes()),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
        // No command link at all.
        assert_eq!(
            action_for_command(None, CommandStatus::Implemented, &Enabled::yes()),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
    }

    #[test]
    fn a_disabled_implemented_command_shows_its_reason() {
        let id = Some(CommandId::parse("edit.undo").expect("id"));
        assert_eq!(
            action_for_command(id, CommandStatus::Implemented, &Enabled::no("no document")),
            Action::Disabled {
                reason: "no document".to_owned()
            }
        );
        // An implemented command disabled without a reason falls back
        // to the not-yet-implemented tooltip.
        assert_eq!(
            action_for_command(id, CommandStatus::Implemented, &Enabled::default()),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
    }

    #[test]
    fn a_stub_reason_is_never_leaked_as_a_tooltip() {
        // A registered stub reports `Enabled::no("not yet
        // implemented")` in lower case; the tooltip is ours.
        let id = Some(CommandId::parse("file.save").expect("id"));
        assert_eq!(
            action_for_command(id, CommandStatus::Stub, &Enabled::no("not yet implemented")),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
    }

    #[test]
    fn the_registry_rule_applies_to_a_control() {
        let empty = Registry::new();
        let app = AppCtx::new(Settings::default());
        // A control with a command link is still a stub with an empty
        // registry (nothing is registered until P4-02).
        let mapped = control("FileSave", "Save", ControlType::Button, Some("file.save"));
        assert_eq!(
            action_for(&mapped, &empty, &app),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
        // A control with no command link.
        let unmapped = control("Save", "Save", ControlType::Button, None);
        assert_eq!(
            action_for(&unmapped, &empty, &app),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
        // A link that is not a valid command id.
        let broken = control("Save", "Save", ControlType::Button, Some("Mystery.Save"));
        assert_eq!(
            action_for(&broken, &empty, &app),
            Action::Disabled {
                reason: NOT_YET_IMPLEMENTED.to_owned()
            }
        );
    }

    #[test]
    fn an_out_of_scope_control_says_so() {
        let empty = Registry::new();
        let app = AppCtx::new(Settings::default());
        let macro_button = control(
            "RunMyMacro",
            "Macro",
            ControlType::Button,
            Some("file.save"),
        );
        assert_eq!(
            action_for(&macro_button, &empty, &app),
            Action::Disabled {
                reason: OUT_OF_SCOPE.to_owned()
            }
        );
    }

    // --- placements -------------------------------------------------

    /// The placements in a tree, counted recursively.
    fn count_placements(placements: &[Placement<'_>]) -> usize {
        placements
            .iter()
            .map(|placement| 1 + count_placements(&placement.children))
            .sum()
    }

    #[test]
    fn repeats_with_identical_id_label_and_order_render_once() {
        let controls = vec![
            control("Save", "Save", ControlType::Button, None),
            Control {
                order: 1,
                ..control("Save", "Save", ControlType::Button, None)
            },
            control("Save", "Save", ControlType::Button, None),
        ];
        let placements = placements(&controls);
        // The first and the one with a different order survive; the
        // exact repeat is dropped (first wins).
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].control.order, 0);
        assert_eq!(placements[1].control.order, 1);
    }

    #[test]
    fn children_render_inside_their_parent() {
        let controls = vec![
            control("PasteMenu", "Paste", ControlType::Menu, None),
            child("Paste", "Paste", ControlType::Button, None, "PasteMenu", 1),
            child(
                "PasteSpecial",
                "Paste Special",
                ControlType::Button,
                None,
                "PasteMenu",
                2,
            ),
            control("Cut", "Cut", ControlType::Button, None),
        ];
        let placements = placements(&controls);
        assert_eq!(placements.len(), 2);
        assert_eq!(placements[0].control.id, FluentId::new("PasteMenu"));
        assert_eq!(placements[0].children.len(), 2);
        assert_eq!(placements[1].control.id, FluentId::new("Cut"));
    }

    #[test]
    fn a_child_of_an_absent_parent_renders_as_a_root() {
        let controls = vec![
            child("Paste", "Paste", ControlType::Button, None, "Ghost", 1),
            control("Cut", "Cut", ControlType::Button, None),
        ];
        let placements = placements(&controls);
        assert_eq!(placements.len(), 2);
    }

    #[test]
    fn a_parent_cycle_renders_every_control_once() {
        let controls = vec![
            child("A", "A", ControlType::Button, None, "B", 0),
            child("B", "B", ControlType::Button, None, "A", 1),
        ];
        let placements = placements(&controls);
        assert_eq!(count_placements(&placements), 2);
    }

    #[test]
    fn deep_nesting_is_bounded_and_loses_no_placement() {
        // A chain deeper than the recursion cap: the last links come
        // back as roots instead of being dropped.
        let mut controls = Vec::new();
        for depth in 0..(MAX_DEPTH + 2) {
            let parent = format!("C{depth}");
            let id = format!("C{}", depth + 1);
            controls.push(child(&id, &id, ControlType::Button, None, &parent, depth));
        }
        let placements = placements(&controls);
        assert_eq!(
            count_placements(&placements),
            usize::try_from(MAX_DEPTH).expect("small") + 2
        );
    }

    #[test]
    fn a_self_parent_renders_once() {
        let controls = vec![child("A", "A", ControlType::Button, None, "A", 0)];
        let placements = placements(&controls);
        assert_eq!(placements.len(), 1);
        assert!(placements[0].children.is_empty());
    }

    // --- rendering: the control-to-command states -------------------

    #[test]
    fn a_stub_control_renders_disabled_with_the_not_yet_implemented_tooltip() {
        let mut set = set_with(vec![
            control("FileSave", "Save", ControlType::Button, Some("file.save")),
            control("SaveLabel", "Saved", ControlType::LabelControl, None),
        ]);
        set.layout.quick_access_toolbar = QuickAccessToolbar {
            controls: vec![control("QSave", "QSave", ControlType::Button, None)],
        };
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::Registry);
        assert!(rig.take_intents().is_empty());
        let widgets = rig.widgets();
        let save = widgets
            .iter()
            .find(|w| w.name == "Save")
            .expect("the Save button renders");
        assert_eq!(save.role, Role::Button);
        assert!(save.disabled, "the stub renders disabled");
        assert_eq!(
            save.description.as_deref(),
            Some(NOT_YET_IMPLEMENTED),
            "the tooltip is ours (ADR 0031)"
        );
        // The label placement renders beside it, with no tooltip.
        let label = widgets
            .iter()
            .find(|w| w.name == "Saved")
            .expect("the label renders");
        assert_eq!(label.role, Role::Label);
        assert!(!label.disabled);
        assert_eq!(label.description, None);
        // A disabled button produces no intent when clicked.
        rig.click(Role::Button, "Save");
        assert!(rig.take_intents().is_empty());
    }

    #[test]
    fn an_enabled_implemented_control_produces_its_intent_on_click() {
        let set = set_with(vec![control(
            "FileSave",
            "Save",
            ControlType::Button,
            Some("file.save"),
        )]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("file.save")));
        rig.run();
        // The enabled control is not disabled and carries no tooltip.
        let save = rig
            .widgets()
            .into_iter()
            .find(|w| w.name == "Save")
            .expect("the Save button renders");
        assert!(!save.disabled);
        assert_eq!(save.description, None);
        rig.click(Role::Button, "Save");
        assert_eq!(rig.take_intents(), vec![intent("file.save")]);
    }

    #[test]
    fn a_disabled_implemented_control_greys_out_with_the_reason() {
        let set = set_with(vec![control(
            "EditUndo",
            "Undo",
            ControlType::Button,
            Some("edit.undo"),
        )]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Disabled {
            reason: "no document".to_owned(),
        });
        rig.run();
        let undo = rig
            .widgets()
            .into_iter()
            .find(|w| w.name == "Undo")
            .expect("the Undo button renders");
        assert!(undo.disabled);
        assert_eq!(undo.description.as_deref(), Some("no document"));
    }

    #[test]
    fn a_checkbox_toggles_and_dispatches() {
        let set = set_with(vec![control(
            "SnapToGrid",
            "Snap to grid",
            ControlType::CheckBox,
            Some("view.snap_to_grid"),
        )]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("view.snap_to_grid")));
        rig.run();
        rig.click(Role::CheckBox, "Snap to grid");
        assert_eq!(rig.take_intents(), vec![intent("view.snap_to_grid")]);
        assert!(rig.tab_bar().toggles.contains(&FluentId::new("SnapToGrid")));
        rig.click(Role::CheckBox, "Snap to grid");
        assert!(!rig.tab_bar().toggles.contains(&FluentId::new("SnapToGrid")));
    }

    // --- rendering: the widget mapping ------------------------------

    #[test]
    fn every_widget_kind_renders_its_placement() {
        let set = set_with(vec![
            control("Save", "Save", ControlType::Button, None),
            control("Bold", "Bold", ControlType::ToggleButton, None),
            control("Grid", "Grid", ControlType::CheckBox, None),
            control(
                "PasteSplit",
                "Paste",
                ControlType::SplitButton,
                Some("edit.paste"),
            ),
            control("PasteGallery", "Paste", ControlType::Gallery, None),
            control("LineStyle", "Line", ControlType::Menu, None),
            control("Zoom", "Zoom", ControlType::DropDown, None),
            control("FontName", "Font", ControlType::ComboBox, None),
            control("FormatPainter", "Painter", ControlType::Control, None),
            control("None0", "Nothing", ControlType::None, None),
            control("Head0", "Heading", ControlType::Category, None),
            control(
                "Launcher",
                "Line dialog",
                ControlType::ButtonDialogBoxLauncher,
                None,
            ),
            child(
                "PasteItem",
                "Paste item",
                ControlType::Button,
                None,
                "PasteGallery",
                1,
            ),
            child(
                "PasteDup",
                "Paste duplicate",
                ControlType::Button,
                None,
                "PasteSplit",
                1,
            ),
        ]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("file.save")));
        rig.run();
        let names = rig.names();
        for name in [
            "TabHome",
            "GroupClipboard",
            "Save",
            "Bold",
            "Grid",
            "Line",
            "Zoom",
            "Font",
            "Painter",
            "Nothing",
            "Heading",
            "Line dialog",
        ] {
            assert!(names.contains_key(name), "missing {name} in {names:?}");
        }
        // Two placements render as "Paste": the split button and the
        // gallery; the split button also has its arrow.
        assert_eq!(names.get("Paste"), Some(&2));
        assert_eq!(names.get(DROPDOWN_TIP), Some(&1));
        // The dropdown items are inside the closed dropdowns.
        assert!(!names.contains_key("Paste item"));
        assert!(!names.contains_key("Paste duplicate"));
    }

    #[test]
    fn a_dropdown_lists_its_item_labels() {
        let set = set_with(vec![
            control("PasteGallery", "Paste", ControlType::Gallery, None),
            child(
                "Paste",
                "Paste",
                ControlType::Button,
                None,
                "PasteGallery",
                1,
            ),
            child(
                "PasteSpecial",
                "Paste Special",
                ControlType::Button,
                None,
                "PasteGallery",
                2,
            ),
            // An exact repeat of the first item: renders once (first
            // wins).
            child(
                "Paste",
                "Paste",
                ControlType::Button,
                None,
                "PasteGallery",
                1,
            ),
        ]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("edit.paste")));
        rig.run();
        assert!(!rig.names().contains_key("Paste Special"));
        rig.click(Role::Button, "Paste");
        let names = rig.names();
        assert_eq!(names.get("Paste"), Some(&2), "the button and its item");
        assert_eq!(names.get("Paste Special"), Some(&1));
    }

    #[test]
    fn a_split_button_is_a_button_plus_an_arrow() {
        let set = set_with(vec![
            control(
                "PasteSplit",
                "Paste",
                ControlType::SplitButton,
                Some("edit.paste"),
            ),
            child("Paste", "Paste", ControlType::Button, None, "PasteSplit", 1),
        ]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("edit.paste")));
        rig.run();
        let names = rig.names();
        assert_eq!(names.get("Paste"), Some(&1));
        assert_eq!(names.get(DROPDOWN_TIP), Some(&1));
        // The arrow opens the items; the button dispatches.
        rig.click(Role::Button, DROPDOWN_TIP);
        assert_eq!(rig.names().get("Paste"), Some(&2));
        rig.click(Role::Button, "Paste");
        assert_eq!(rig.take_intents(), vec![intent("edit.paste")]);
    }

    #[test]
    fn a_toggle_button_flips_its_display_state() {
        let set = set_with(vec![control(
            "Bold",
            "Bold",
            ControlType::ToggleButton,
            None,
        )]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("format.text.bold")));
        rig.run();
        rig.click(Role::Button, "Bold");
        assert!(rig.tab_bar().toggles.contains(&FluentId::new("Bold")));
    }

    #[test]
    fn a_structural_control_stacks_its_children() {
        let set = set_with(vec![
            control("Recent", "Recent", ControlType::TaskFormGroup, None),
            child("Open", "Open", ControlType::Button, None, "Recent", 1),
            child(
                "RecentGroup",
                "Places",
                ControlType::Group,
                None,
                "Recent",
                2,
            ),
            child(
                "Browse",
                "Browse",
                ControlType::Button,
                None,
                "RecentGroup",
                1,
            ),
        ]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("file.open")));
        rig.run();
        let names = rig.names();
        // Structural children always render, at any depth.
        for name in ["Recent", "Open", "Places", "Browse"] {
            assert!(names.contains_key(name), "missing {name} in {names:?}");
        }
    }

    #[test]
    fn the_quick_access_toolbar_renders_its_controls() {
        let mut set = set_with(Vec::new());
        set.layout.quick_access_toolbar = QuickAccessToolbar {
            controls: vec![
                control("UndoQuickAccess", "UndoQ", ControlType::Button, None),
                control("SaveQuickAccess", "SaveQ", ControlType::Button, None),
            ],
        };
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::QuickAccess);
        rig.set_action(Action::Emit(intent("edit.undo")));
        rig.run();
        let names = rig.names();
        assert_eq!(names.get("UndoQ"), Some(&1));
        assert_eq!(names.get("SaveQ"), Some(&1));
        rig.click(Role::Button, "UndoQ");
        assert_eq!(rig.take_intents(), vec![intent("edit.undo")]);
    }

    #[test]
    fn the_backstage_stacks_its_tabs_and_controls() {
        let mut set = set_with(Vec::new());
        set.layout.backstage = Backstage {
            tabs: vec![
                BackstageTab {
                    id: FluentId::new("TabInfo"),
                    label: "TabInfo".to_owned(),
                    order: 0,
                    bounds: None,
                    icon_ref: None,
                    keytip: None,
                    size_variant: None,
                    visual_notes: None,
                    controls: vec![control("FileSave", "FileSave", ControlType::Button, None)],
                },
                BackstageTab {
                    id: FluentId::new("TabRecent"),
                    label: "TabRecent".to_owned(),
                    order: 1,
                    bounds: None,
                    icon_ref: None,
                    keytip: None,
                    size_variant: None,
                    visual_notes: None,
                    controls: vec![
                        child(
                            "RecentGroup",
                            "RecentGroup",
                            ControlType::Group,
                            None,
                            "Ghost",
                            0,
                        ),
                        control("OpenBrowse", "OpenBrowse", ControlType::Button, None),
                    ],
                },
            ],
        };
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::Backstage);
        assert!(rig.names().is_empty(), "a closed backstage draws nothing");

        rig.tab_bar().set_backstage_open(true);
        rig.run();
        let names = rig.names();
        assert_eq!(names.get("TabInfo"), Some(&1));
        assert_eq!(names.get("TabRecent"), Some(&1));
        assert_eq!(
            names.get("FileSave"),
            Some(&1),
            "the active entry's control"
        );
        assert!(
            !names.contains_key("OpenBrowse"),
            "the other entry is hidden"
        );

        rig.tab_bar().set_active_backstage_tab(1);
        rig.run();
        let names = rig.names();
        assert_eq!(names.get("OpenBrowse"), Some(&1));
        assert_eq!(
            names.get("RecentGroup"),
            Some(&1),
            "an orphan renders as a root"
        );
        assert!(!names.contains_key("FileSave"));
    }

    #[test]
    fn a_context_menu_renders_only_when_selected() {
        let mut set = set_with(Vec::new());
        set.layout.context_menus = vec![ContextMenu {
            id: FluentId::new("ShapeShortcutMenu"),
            label: "ShapeShortcutMenu".to_owned(),
            order: 0,
            bounds: None,
            icon_ref: None,
            keytip: None,
            size_variant: None,
            visual_notes: None,
            controls: vec![control("Cut", "Cut", ControlType::Button, Some("edit.cut"))],
        }];
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::ContextMenu);
        assert!(rig.names().is_empty(), "no menu selected draws nothing");

        rig.tab_bar()
            .set_context_menu(Some(FluentId::new("ShapeShortcutMenu")));
        rig.set_action(Action::Emit(intent("edit.cut")));
        rig.run();
        assert_eq!(rig.names().get("Cut"), Some(&1));
        rig.click(Role::Button, "Cut");
        assert_eq!(rig.take_intents(), vec![intent("edit.cut")]);

        // A menu id the layout does not have draws nothing (no crash).
        rig.tab_bar()
            .set_context_menu(Some(FluentId::new("GhostMenu")));
        rig.run();
        assert!(rig.names().is_empty());
    }

    #[test]
    fn a_group_caption_holds_the_mark_the_label_and_the_launcher() {
        let set = set_with(vec![
            control("Save", "Save", ControlType::Button, None),
            control(
                "Launcher",
                "Line dialog",
                ControlType::ButtonDialogBoxLauncher,
                None,
            ),
        ]);
        let mut rig = Rig::new(LayoutKind::Panel2016, set, Surface::CommandBar);
        rig.set_action(Action::Emit(intent("format.line")));
        rig.run();
        assert_eq!(rig.names().get("GroupClipboard"), Some(&1));
        assert_eq!(rig.names().get("Line dialog"), Some(&1));
        rig.click(Role::Button, "Line dialog");
        assert_eq!(rig.take_intents(), vec![intent("format.line")]);
    }

    // --- the committed layouts -------------------------------------

    #[test]
    fn both_layouts_render_one_widget_per_placement() {
        for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
            let set = sets().get(kind);
            let mut rig = Rig::new(kind, set.clone(), Surface::CommandBarAndQuickAccess);
            rig.set_action(Action::Emit(intent("file.save")));
            rig.run();

            // The command bar and quick access are drawn together, like the
            // shell draws them.
            let qat = expected_names(&set.layout.quick_access_toolbar.controls, false);

            // Every command-bar tab: one widget per tab, per group and per
            // root control of the active tab's groups.
            for (index, tab) in set.layout.tabs.iter().enumerate() {
                rig.tab_bar().set_active_tab(index);
                rig.run();
                let mut expected = vec![FILE_BUTTON.to_owned()];
                expected.extend(set.layout.tabs.iter().map(|t| t.label.clone()));
                for group in &tab.groups {
                    expected.push(group.label.clone());
                    expected.extend(expected_names(&group.controls, false));
                }
                expected.extend(qat.iter().cloned());
                assert_eq!(
                    rig.names(),
                    counts_of(&expected),
                    "{kind:?} command-bar tab {index} ({})",
                    tab.label
                );
            }

            // Every backstage entry (its own rig: the backstage is a
            // full-window stack): one widget per entry and per root
            // control of the active entry.
            let mut rig = Rig::new(kind, set.clone(), Surface::Backstage);
            rig.tab_bar().set_backstage_open(true);
            rig.set_action(Action::Emit(intent("file.save")));
            rig.run();
            for (index, tab) in set.layout.backstage.tabs.iter().enumerate() {
                rig.tab_bar().set_active_backstage_tab(index);
                rig.run();
                let mut expected = set
                    .layout
                    .backstage
                    .tabs
                    .iter()
                    .map(|t| t.label.clone())
                    .collect::<Vec<_>>();
                expected.extend(expected_names(&tab.controls, false));
                assert_eq!(
                    rig.names(),
                    counts_of(&expected),
                    "{kind:?} backstage entry {index} ({})",
                    tab.label
                );
            }
        }
    }

    #[test]
    fn the_layout_counts_are_pinned() {
        let mut report = String::new();
        for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
            report.push_str(&counts_report(kind, sets().get(kind)));
            report.push('\n');
        }
        insta::assert_snapshot!("layout_counts", report);
    }

    #[test]
    fn every_dropdown_of_the_2016_command_bar_lists_its_items() {
        // Every tab of the committed 2016 layout: open each dropdown
        // root and check the added widgets are exactly its nested
        // controls (ADR 0031).
        let set = sets().get(LayoutKind::Panel2016);
        let mut rig = Rig::committed(LayoutKind::Panel2016);
        rig.set_action(Action::Emit(intent("file.save")));
        rig.run();
        for (index, tab) in set.layout.tabs.iter().enumerate() {
            rig.tab_bar().set_active_tab(index);
            rig.run();
            for group in &tab.groups {
                let kept = expected_kept(&group.controls);
                for control in &kept {
                    if !is_dropdown(control.r#type) {
                        continue;
                    }
                    let children = children_of(&kept, control.id.as_str());
                    if children.is_empty() {
                        continue;
                    }
                    let before = rig.names();
                    click_dropdown(&mut rig, control);
                    let added = added_names(&before, &rig.names());
                    let mut want = Vec::new();
                    for child in &children {
                        let grand = children_of(&kept, child.id.as_str());
                        want.extend(widget_names(child, grand.len()));
                    }
                    want.sort();
                    assert_eq!(
                        added,
                        want,
                        "{:?} tab {} group {} control {}",
                        kind_label(LayoutKind::Panel2016),
                        tab.label,
                        group.label,
                        control.label
                    );
                    // Close the dropdown by clicking outside it.
                    rig.click(Role::Button, FILE_BUTTON);
                }
            }
        }
    }

    #[test]
    fn switching_layouts_rebuilds_the_tab_set() {
        // ADR 0032: dispatching `view.ui_layout.panel_365` switches
        // the layout without a restart. The dispatch is the shell's
        // (P4-03) and the command's (P4-02); here the intent is built
        // by hand and its documented effect applied to the same
        // command bar in the same egui context.
        let dispatch = intent("view.ui_layout.panel_365");
        let mut rig = Rig::switching(LayoutKind::Panel2016);
        rig.set_action(Action::Emit(intent("file.save")));
        rig.run();
        let tabs_2016: Vec<String> = sets()
            .panel_2016
            .layout
            .tabs
            .iter()
            .map(|t| t.label.clone())
            .collect();
        let tabs_365: Vec<String> = sets()
            .panel_365
            .layout
            .tabs
            .iter()
            .map(|t| t.label.clone())
            .collect();
        assert_ne!(tabs_2016, tabs_365, "the two layouts differ");
        for label in &tabs_2016 {
            assert!(rig.names().contains_key(label), "2016 tab {label}");
        }

        // Apply the intent's effect (ADR 0032).
        assert_eq!(dispatch.command.as_str(), "view.ui_layout.panel_365");
        let _ = dispatch;
        rig.tab_bar().set_kind(LayoutKind::Panel365);
        rig.run();
        for label in &tabs_365 {
            assert!(rig.names().contains_key(label), "365 tab {label}");
        }
        assert_eq!(rig.tab_bar().kind(), LayoutKind::Panel365);
        assert_eq!(rig.tab_bar().active_tab(), 0, "switching resets the tab");
    }

    #[test]
    fn the_file_button_toggles_the_backstage() {
        let mut rig = Rig::committed(LayoutKind::Panel2016);
        assert!(!rig.tab_bar().backstage_open());
        rig.click(Role::Button, FILE_BUTTON);
        assert!(rig.tab_bar().backstage_open());
        rig.click(Role::Button, FILE_BUTTON);
        assert!(!rig.tab_bar().backstage_open());
    }

    #[test]
    fn clicking_a_tab_activates_it() {
        let mut rig = Rig::committed(LayoutKind::Panel2016);
        let second = sets().panel_2016.layout.tabs[1].label.clone();
        rig.click(Role::Button, &second);
        assert_eq!(rig.tab_bar().active_tab(), 1);
    }

    #[test]
    fn the_builtin_registry_enables_its_command_bar_controls() {
        // P4-04 was written against the registry stub; with P4-02's
        // builtins every command bar control bound to `view.toggle_grid` or
        // `view.zoom.fit` resolves to an implemented command, enabled
        // once a document is open.
        let registry = larrygram_commands::builtins::registry().expect("registry");
        let mut app = AppCtx::new(Settings::default());
        // One rectangle on a new page: `view.zoom.fit` needs a page.
        let mut document = larrygram_model::Document::new();
        let master = larrygram_model::stencils::basic_shapes().masters[0].clone();
        larrygram_model::stencils::instantiate(&mut document, &master);
        app.open_document(larrygram_commands::DocumentSession::new(document, None));
        for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
            let mut bound = 0;
            for tab in &sets().get(kind).layout.tabs {
                for group in &tab.groups {
                    for control in &group.controls {
                        let command = control.command.as_ref().map(|id| id.as_str());
                        if matches!(command, Some("view.toggle_grid" | "view.zoom.fit")) {
                            bound += 1;
                            assert!(
                                matches!(action_for(control, &registry, &app), Action::Emit(_)),
                                "{kind:?} {} is not enabled",
                                control.id.as_str()
                            );
                        }
                    }
                }
            }
            assert!(bound >= 2, "{kind:?}: {bound} bound controls");
        }
    }

    #[test]
    fn with_ui_renders_a_whole_command_bar_without_panicking() {
        // Smoke test on the plain egui test ui (no harness).
        let set = sets().get(LayoutKind::Panel365).clone();
        let mut tab_bar = TabBar::new(LayoutKind::Panel365);
        let registry = Registry::new();
        let app = AppCtx::new(Settings::default());
        with_ui(|ui| {
            let intents = tab_bar.show(ui, &set, &registry, &app).expect("renders");
            assert!(intents.is_empty());
            let intents = tab_bar
                .show_quick_access(ui, &set, &registry, &app)
                .expect("renders");
            assert!(intents.is_empty());
            tab_bar.set_backstage_open(true);
            let intents = tab_bar
                .show_backstage(ui, &set, &registry, &app)
                .expect("renders");
            assert!(intents.is_empty());
            let intents = tab_bar
                .show_context_menu(ui, &set, &registry, &app)
                .expect("renders");
            assert!(intents.is_empty());
        });
    }
}
