//! Toolbar painting for the Panel Classic widgets
//! (`docs/ui/panel-classic.md` "Toolbars"): docked rows with
//! grippers, split buttons, `split_recent`, palettes, colour
//! dropdowns and combos.
//!
//! Colours come from the egui defaults until task P7-05 merges the
//! theme tokens; the metrics below are the layout's numbers.

use egui::{Id, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo};

use larrygram_commands::{Args, Source};
use larrygram_ui_spec::v2::{DisplayMode, Dock, Item, ItemKind};

use crate::Intent;
use crate::icons;
use crate::panel_classic::menu::{self, MenuOutcome};
use crate::panel_classic::rows::{self, Activation, Row};
use crate::panel_classic::{MAX_DEPTH, PanelClassic, PanelInput};

/// Height of one toolbar row (`docs/ui/panel-classic.md`).
pub(crate) const ROW: f32 = 26.0;

/// Height of a toolbar button.
const BUTTON_HEIGHT: f32 = 22.0;

/// Width of a toolbar gripper.
const GRIPPER: f32 = 7.0;

/// Width of a split button's arrow part.
const SPLIT_ARROW: f32 = 11.0;

/// The id of the right-click toolbar list popup.
const TOOLBAR_LIST: &str = "panel_classic.toolbar_list";

/// Draws the docked toolbar rows and the floating toolbars, and the
/// open toolbar popup (`split`, `split_recent`, `dropdown` and
/// `palette` contents).
pub(crate) fn draw_toolbars(
    ui: &mut Ui,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    intents: &mut Vec<Intent>,
) {
    let mut secondary = false;
    let mut anchor = Rect::NOTHING;
    let toolbars = toolbars_for(input, state);
    let mut by_row: Vec<(u32, Vec<&larrygram_ui_spec::v2::Toolbar>)> = Vec::new();
    for toolbar in toolbars {
        let row = toolbar.row.unwrap_or(0);
        match by_row.iter_mut().find(|(existing, _)| *existing == row) {
            Some((_, list)) => list.push(toolbar),
            None => by_row.push((row, vec![toolbar])),
        }
    }
    by_row.sort_by_key(|(row, _)| *row);
    for (_, row_toolbars) in by_row {
        ui.horizontal_wrapped(|ui| {
            ui.set_min_height(ROW);
            for toolbar in row_toolbars {
                draw_gripper(ui, &toolbar.id);
                for item in &toolbar.items {
                    if item.hidden {
                        continue;
                    }
                    draw_item(ui, item, input, state, intents, &mut secondary, &mut anchor);
                }
            }
        });
    }
    // Floating toolbars (`dock: floating`) as small tool windows.
    for toolbar in input
        .layout
        .toolbars
        .iter()
        .filter(|toolbar| toolbar.dock == Dock::Floating)
    {
        if !state.toolbar_visible_in(input.layout, input.mode, &toolbar.id) {
            continue;
        }
        let mut open = true;
        let label = toolbar.label.clone().unwrap_or_else(|| toolbar.id.clone());
        egui::Window::new(label)
            .id(Id::unique(("panel_classic_floating", toolbar.id.clone())))
            // Near the top left of the drawing, clear of the menu bar
            // and the docked rows (`docs/ui/panel-classic.md`).
            .default_pos(Pos2::new(300.0, 120.0))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                for item in &toolbar.items {
                    if item.hidden {
                        continue;
                    }
                    draw_item(ui, item, input, state, intents, &mut secondary, &mut anchor);
                }
            });
        if !open {
            state.set_toolbar_visible_in(input.mode, &toolbar.id, false);
        }
    }
    if secondary {
        state.open_popup = Some(TOOLBAR_LIST.to_owned());
        state.popup_rect = anchor;
    }
    draw_popup(ui, input, state, intents);
}

/// The toolbars drawn in `mode`: the general toolbars plus the mode's
/// own (`shown_in`), each only when visible.
fn toolbars_for<'a>(
    input: &PanelInput<'a>,
    state: &PanelClassic,
) -> Vec<&'a larrygram_ui_spec::v2::Toolbar> {
    input
        .layout
        .toolbars
        .iter()
        .filter(|toolbar| {
            toolbar.dock != Dock::Floating
                && (toolbar.shown_in.is_none() || toolbar.shown_in == input.mode.context())
                && state.toolbar_visible_in(input.layout, input.mode, &toolbar.id)
        })
        .collect()
}

/// The 7 px gripper at the start of every toolbar.
fn draw_gripper(ui: &mut Ui, id: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(GRIPPER, BUTTON_HEIGHT), Sense::hover());
    let painter = ui.painter();
    let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    for offset in [0.0, 3.0] {
        painter.line_segment(
            [
                Pos2::new(rect.left() + 2.0 + offset, rect.top() + 4.0),
                Pos2::new(rect.left() + 2.0 + offset, rect.bottom() - 4.0),
            ],
            egui::Stroke::new(1.0, color),
        );
    }
    let _ = id;
}

/// One toolbar item: `begin_group` separator, then the kind's widget.
fn draw_item(
    ui: &mut Ui,
    item: &Item,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    intents: &mut Vec<Intent>,
    secondary: &mut bool,
    anchor: &mut Rect,
) {
    if item.begin_group {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, BUTTON_HEIGHT), Sense::hover());
        let y = rect.center().y;
        ui.painter().line_segment(
            [
                Pos2::new(rect.center().x, y - 8.0),
                Pos2::new(rect.center().x, y + 8.0),
            ],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
    }
    match item.kind {
        ItemKind::Combo => draw_combo(ui, item, input, state, intents),
        ItemKind::Split | ItemKind::SplitRecent => {
            draw_split(ui, item, input, state, intents, secondary, anchor)
        }
        ItemKind::Dropdown | ItemKind::Palette => {
            draw_dropdown(ui, item, input, state, intents, secondary, anchor)
        }
        _ => draw_button(ui, item, input, state, intents, secondary, anchor),
    }
}

/// A toolbar button (`button`, `check`, `toggle`): icon, label or
/// both per `display`, latched from the command's `checked` state.
fn draw_button(
    ui: &mut Ui,
    item: &Item,
    input: &PanelInput<'_>,
    state: &PanelClassic,
    intents: &mut Vec<Intent>,
    secondary: &mut bool,
    anchor: &mut Rect,
) {
    let activation = rows::button_activation(item, input, Source::Ui, Args::new());
    let enabled = !matches!(activation, Activation::Disabled { .. });
    let label = item.label.clone().unwrap_or_default();
    let checked = rows::item_check(item, input, state, false).unwrap_or(false);
    let response = match button_content(item, &label, checked) {
        Some(button) => {
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(Vec2::new(23.0, BUTTON_HEIGHT), button)
            })
            .inner
        }
        None => {
            let button = egui::Button::new(label.clone()).selected(checked);
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(Vec2::new(48.0, BUTTON_HEIGHT), button)
            })
            .inner
        }
    };
    finish_button(
        response,
        item,
        &label,
        &activation,
        intents,
        secondary,
        anchor,
    );
}

/// The content of a toolbar button per `display` (`icon`, `text`,
/// `icon_text`): `None` shows the label only.
fn button_content(item: &Item, label: &str, checked: bool) -> Option<egui::Button<'static>> {
    let display = item.display.unwrap_or(DisplayMode::Icon);
    match (display, item.icon.as_deref()) {
        (DisplayMode::Text, _) | (_, None) => None,
        (DisplayMode::Icon, Some(icon)) => {
            Some(egui::Button::new(icons::icon(icon, menu::ICON)).selected(checked))
        }
        (DisplayMode::IconText, Some(icon)) => Some(
            egui::Button::new((icons::icon(icon, menu::ICON), label.to_owned())).selected(checked),
        ),
    }
}

/// The shared tail of every toolbar button: tooltip, dispatch and the
/// right-click toolbar list.
fn finish_button(
    response: egui::Response,
    item: &Item,
    label: &str,
    activation: &Activation,
    intents: &mut Vec<Intent>,
    secondary: &mut bool,
    anchor: &mut Rect,
) {
    let enabled = !matches!(activation, Activation::Disabled { .. });
    let mut response = response;
    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::Button, enabled, label.to_owned())
    });
    let tooltip = rows::button_tooltip(label, item.shortcut.as_deref(), activation);
    if !tooltip.is_empty() {
        response = response.on_hover_text(tooltip);
    }
    if response.secondary_clicked() {
        *secondary = true;
        *anchor = Rect::from_min_size(response.rect.min, Vec2::new(180.0, 200.0));
    }
    if response.clicked()
        && let Activation::Intent(intent) = activation
    {
        intents.push(intent.clone());
    }
}

/// A split button: the main part runs `command` (or the last child
/// chosen, for `split_recent`), the 11 px arrow part opens `items`.
fn draw_split(
    ui: &mut Ui,
    item: &Item,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    intents: &mut Vec<Intent>,
    secondary: &mut bool,
    anchor: &mut Rect,
) {
    let label = item.label.clone().unwrap_or_default();
    // `split_recent`: the main part shows and runs the last child
    // chosen, else the item's own command.
    let recent = state.split_recent.get(&item.id).cloned();
    let (main_item, main_activation) = match (&item.kind, recent) {
        (ItemKind::SplitRecent, Some(recent)) => {
            let child = item
                .items
                .iter()
                .find(|child| child.id == recent)
                .unwrap_or(item);
            (
                child.clone(),
                rows::button_activation(child, input, Source::Ui, Args::new()),
            )
        }
        _ => (
            item.clone(),
            rows::button_activation(item, input, Source::Ui, Args::new()),
        ),
    };
    let enabled = !matches!(main_activation, Activation::Disabled { .. });
    let main_label = main_item.label.clone().unwrap_or_else(|| label.clone());
    let main_checked = rows::item_check(&main_item, input, state, false).unwrap_or(false);
    let response = match button_content(&main_item, &main_label, main_checked) {
        Some(button) => {
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(Vec2::new(23.0, BUTTON_HEIGHT), button)
            })
            .inner
        }
        None => {
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    Vec2::new(48.0, BUTTON_HEIGHT),
                    egui::Button::new(main_label.clone()),
                )
            })
            .inner
        }
    };
    finish_button(
        response,
        &main_item,
        &main_label,
        &main_activation,
        intents,
        secondary,
        anchor,
    );
    // The arrow part.
    let arrow = egui::Button::new(icons::dropdown_arrow(menu::ICON))
        .min_size(Vec2::new(SPLIT_ARROW, BUTTON_HEIGHT));
    let response = ui.add_sized(Vec2::new(SPLIT_ARROW, BUTTON_HEIGHT), arrow);
    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::Button, true, format!("{label} menu"))
    });
    if response.clicked() {
        toggle_popup(state, item, response.rect);
    }
    if response.secondary_clicked() {
        *secondary = true;
        *anchor = Rect::from_min_size(response.rect.min, Vec2::new(180.0, 200.0));
    }
}

/// A dropdown or palette button: the whole button opens `items`.
fn draw_dropdown(
    ui: &mut Ui,
    item: &Item,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    _intents: &mut Vec<Intent>,
    secondary: &mut bool,
    anchor: &mut Rect,
) {
    let label = item.label.clone().unwrap_or_default();
    let activation = rows::button_activation(item, input, Source::Ui, Args::new());
    let enabled = !matches!(activation, Activation::Disabled { .. });
    let content = egui::Button::new((
        icons::icon(item.icon.as_deref().unwrap_or(&item.id), menu::ICON),
        icons::dropdown_arrow(10.0),
    ));
    let response = ui.add_enabled_ui(enabled, |ui| {
        ui.add_sized(Vec2::new(34.0, BUTTON_HEIGHT), content)
    });
    let mut response = response.inner;
    response
        .widget_info(|| WidgetInfo::labeled(egui::accesskit::Role::Button, enabled, label.clone()));
    let tooltip = rows::button_tooltip(&label, item.shortcut.as_deref(), &activation);
    if !tooltip.is_empty() {
        response = response.on_hover_text(tooltip);
    }
    if response.clicked() {
        toggle_popup(state, item, response.rect);
    }
    if response.secondary_clicked() {
        *secondary = true;
        *anchor = Rect::from_min_size(response.rect.min, Vec2::new(180.0, 200.0));
    }
}

/// An editable combo (font, size, zoom): applies its text on Enter
/// with the `value` argument, reverts on Esc.
fn draw_combo(
    ui: &mut Ui,
    item: &Item,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    intents: &mut Vec<Intent>,
) {
    let label = item.label.clone().unwrap_or_default();
    let mut buffer = state.combo_text.get(&item.id).cloned().unwrap_or_default();
    let enabled = !matches!(
        rows::combo_activation(item, input, Source::Ui, String::new()),
        Activation::Disabled { .. }
    );
    let width = item.width.unwrap_or(86.0) as f32;
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                Vec2::new(width, BUTTON_HEIGHT),
                egui::TextEdit::singleline(&mut buffer).hint_text(label.clone()),
            )
        })
        .inner;
    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::ComboBox, enabled, label.clone())
    });
    state.combo_text.insert(item.id.clone(), buffer.clone());
    // Enter applies and Esc reverts (egui surrenders text focus on the
    // key press, so the standard `lost_focus` idiom reads the key).
    let lost = response.lost_focus();
    if lost && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Activation::Intent(intent) =
            rows::combo_activation(item, input, Source::Ui, buffer.clone())
        {
            intents.push(intent);
        }
        state.combo_applied.insert(item.id.clone(), buffer);
    } else if lost && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        let applied = state
            .combo_applied
            .get(&item.id)
            .cloned()
            .unwrap_or_default();
        state.combo_text.insert(item.id.clone(), applied);
    }
}

/// Opens or closes the popup of a toolbar item.
fn toggle_popup(state: &mut PanelClassic, item: &Item, rect: Rect) {
    if state.open_popup.as_deref() == Some(item.id.as_str()) {
        state.open_popup = None;
    } else {
        state.open_popup = Some(item.id.clone());
        state.popup_rect = rect;
    }
}

/// Draws the open toolbar popup: the toolbar list (right-click) or
/// the item's `items` (`docs/ui/panel-classic.md`).
fn draw_popup(
    ui: &mut Ui,
    input: &PanelInput<'_>,
    state: &mut PanelClassic,
    intents: &mut Vec<Intent>,
) {
    let Some(open) = state.open_popup.clone() else {
        return;
    };
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.open_popup = None;
        return;
    }
    let mut clicked: Option<Activation> = None;
    if open == TOOLBAR_LIST {
        let rows = toolbar_list_rows(input, state);
        clicked = draw_popup_level(ui, &rows, state.popup_rect.left_bottom(), 0);
    } else {
        let items = input
            .layout
            .toolbars
            .iter()
            .find_map(|toolbar| super::find_item(&toolbar.items, &open))
            .or_else(|| {
                input
                    .layout
                    .menu_bars
                    .iter()
                    .find_map(|bar| find_in_menus(&bar.menus, &open))
            });
        if let Some(item) = items {
            let rows = rows::rows(&item.items, input, state, Source::Ui, false, 0);
            clicked = draw_popup_level(ui, &rows, state.popup_rect.left_bottom(), 0);
        } else {
            state.open_popup = None;
        }
    }
    if let Some(activation) = clicked {
        match activation {
            Activation::Intent(intent) => {
                intents.push(intent);
                state.open_popup = None;
            }
            Activation::ToolbarToggle { toolbar, intent } => {
                if let Some(intent) = intent {
                    intents.push(intent);
                }
                let visible = state.toolbar_visible_in(input.layout, input.mode, &toolbar);
                state.set_toolbar_visible_in(input.mode, &toolbar, !visible);
                state.open_popup = None;
            }
            _ => {}
        }
    }
    if ui.input(|i| i.pointer.any_click())
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
        && !Rect::from_min_size(state.popup_rect.left_bottom(), Vec2::new(220.0, 240.0))
            .contains(pointer)
        && !state.popup_rect.contains(pointer)
    {
        state.open_popup = None;
    }
}

/// The rows of the right-click toolbar list (one row per toolbar,
/// checked when visible).
fn toolbar_list_rows(input: &PanelInput<'_>, state: &PanelClassic) -> Vec<Row> {
    input
        .layout
        .toolbars
        .iter()
        .filter(|toolbar| toolbar.label.is_some())
        .map(|toolbar| {
            let label = toolbar.label.clone().unwrap_or_default();
            let visible = state.toolbar_visible_in(input.layout, input.mode, &toolbar.id);
            Row {
                id: toolbar.id.clone(),
                label: label.clone(),
                access: None,
                shortcut: None,
                icon: None,
                check: Some(visible),
                tooltip: label,
                activation: Activation::ToolbarToggle {
                    toolbar: toolbar.id.clone(),
                    intent: None,
                },
                children: Vec::new(),
                separator: false,
            }
        })
        .collect()
}

/// One popup level and its hovered submenus; returns the activation
/// of the row the user clicked (submenus open on hover).
fn draw_popup_level(ui: &mut Ui, rows: &[Row], pos: Pos2, depth: u32) -> Option<Activation> {
    if depth >= MAX_DEPTH {
        return None;
    }
    let width = menu::menu_width(ui, rows);
    let mut outcome = MenuOutcome::default();
    egui::Area::new(Id::unique((
        "panel_classic_popup",
        depth,
        pos.x.to_bits(),
        pos.y.to_bits(),
    )))
    .order(egui::Order::Foreground)
    .fixed_pos(pos)
    .show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            outcome = menu::draw_menu(ui, rows, width, None, false);
        });
    });
    let mut activated = None;
    if let Some(index) = outcome.hovered
        && let Some(row) = rows.get(index)
        && row.has_children()
        && let Some(rect) = outcome.rects.get(index)
    {
        activated = draw_popup_level(
            ui,
            &row.children,
            Pos2::new(rect.right(), rect.top()),
            depth + 1,
        );
    }
    if let Some(index) = outcome.clicked
        && let Some(row) = rows.get(index)
        && !row.has_children()
    {
        activated = Some(row.activation.clone());
    }
    activated
}

/// The items of a menu tree, searched by placement id.
fn find_in_menus<'a>(menus: &'a [larrygram_ui_spec::v2::Menu], id: &str) -> Option<&'a Item> {
    menus
        .iter()
        .find_map(|menu| super::find_item(&menu.items, id))
}
