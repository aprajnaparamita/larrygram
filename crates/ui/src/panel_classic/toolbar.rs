//! Toolbar painting for the Panel Classic widgets
//! (`docs/ui/panel-classic.md` "Toolbars"): docked rows with
//! grippers, split buttons, `split_recent`, palettes, colour
//! dropdowns and combos.
//!
//! Colours come from the theme by token name (ADR 0063,
//! [`crate::theme::color_at`]) and the sizes from
//! [`ClassicMetrics`]; with no theme applied the defaults draw.

use egui::{Id, Pos2, Rect, Sense, Ui, Vec2, WidgetInfo};

use larrygram_commands::{Args, Source};
use larrygram_ui_spec::v2::{DisplayMode, Dock, Item, ItemKind};

use crate::Intent;
use crate::icons;
use crate::panel_classic::menu::{self, MenuOutcome};
use crate::panel_classic::metrics::ClassicMetrics;
use crate::panel_classic::rows::{self, Activation, Row};
use crate::panel_classic::{MAX_DEPTH, PanelClassic, PanelInput};

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
            let m = ClassicMetrics::for_ctx(ui.ctx());
            ui.set_min_height(m.toolbar_row_height);
            // Toolbars sit end to end; one that does not fit starts a
            // new row, and one wider than the window breaks between
            // its items (`docs/ui/panel-classic.md`; task P7-17: egui
            // wrapped single widgets, so a split's arrow landed on the
            // next row and the last button ran off the window). Each
            // item's width is the one it took last frame, or the
            // estimate on the first frame.
            let room = ui.available_width();
            let spacing = ui.spacing().item_spacing.x;
            let mut x = 0.0_f32;
            for toolbar in row_toolbars {
                let items: Vec<&Item> = toolbar.items.iter().filter(|item| !item.hidden).collect();
                let widths: Vec<f32> = items
                    .iter()
                    .map(|item| {
                        state
                            .toolbar_widths
                            .get(&item.id)
                            .copied()
                            .unwrap_or_else(|| item_width(ui, item, &m) + spacing)
                    })
                    .collect();
                let whole = m.toolbar_gripper_width + spacing + widths.iter().sum::<f32>();
                if x > 0.0 && x + whole > room {
                    ui.end_row();
                    x = 0.0;
                }
                draw_gripper(ui, &toolbar.id);
                x += m.toolbar_gripper_width + spacing;
                for (item, width) in items.into_iter().zip(widths) {
                    if x > 0.0 && x + width > room {
                        ui.end_row();
                        x = 0.0;
                    }
                    let before = ui.cursor().min;
                    draw_item(ui, item, input, state, intents, &mut secondary, &mut anchor);
                    let after = ui.cursor().min;
                    if (after.y - before.y).abs() < 0.5 && after.x > before.x {
                        state
                            .toolbar_widths
                            .insert(item.id.clone(), after.x - before.x);
                    }
                    x += width;
                }
            }
        });
    }
    // Floating toolbars (`dock: floating`) as small tool windows,
    // clear of the docked rows (P7-12 minor 8: they used to land on
    // top of the rows and eat the clicks).
    // Each window opens under the one before it (task P7-17: a 28 px
    // cascade hid the buttons of every window but the last).
    let mut next_y = ui.cursor().min.y + 8.0;
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
        let shown = egui::Window::new(label)
            .id(Id::unique(("panel_classic_floating", toolbar.id.clone())))
            // Below the docked rows, near the top left of the
            // drawing, cascaded so the windows do not cover each
            // other (`docs/ui/panel-classic.md`).
            .default_pos(Pos2::new(300.0, next_y))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                // A small tool window: the buttons in rows of at most
                // [`FLOATING_COLUMNS`] (task P7-17: one vertical strip
                // in a window as wide as the frame).
                let m = ClassicMetrics::for_ctx(ui.ctx());
                let spacing = ui.spacing().item_spacing.x;
                let width = FLOATING_COLUMNS * (m.toolbar_button.x + spacing);
                ui.set_max_width(width);
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(width);
                    for item in &toolbar.items {
                        if item.hidden {
                            continue;
                        }
                        draw_item(ui, item, input, state, intents, &mut secondary, &mut anchor);
                    }
                });
            });
        if let Some(shown) = shown {
            next_y = shown.response.rect.max.y + 8.0;
        }
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

/// The most buttons in one row of a floating toolbar window.
const FLOATING_COLUMNS: f32 = 6.0;

/// The width one docked toolbar item takes, separator included,
/// before the item spacing: the sizes `draw_item` gives each kind.
fn item_width(ui: &Ui, item: &Item, m: &ClassicMetrics) -> f32 {
    let separator = if item.begin_group {
        m.toolbar_separator_width + 2.0 * m.toolbar_separator_margin + ui.spacing().item_spacing.x
    } else {
        0.0
    };
    let body = match item.kind {
        ItemKind::Combo => item
            .width
            .map(|width| width as f32)
            .filter(|width| width.is_finite() && *width > 0.0)
            .unwrap_or_else(|| combo_width(&item.id, m)),
        ItemKind::Split | ItemKind::SplitRecent => {
            button_width(ui, item, m) + ui.spacing().item_spacing.x + m.toolbar_split_arrow_width
        }
        ItemKind::Dropdown | ItemKind::Palette => m.toolbar_button.x + m.toolbar_split_arrow_width,
        _ => button_width(ui, item, m),
    };
    separator + body.min(1000.0)
}

/// The width of a toolbar button's main part per `display`: the icon
/// cell, the icon and label, or the label alone (two cells).
fn button_width(ui: &Ui, item: &Item, m: &ClassicMetrics) -> f32 {
    let display = item.display.unwrap_or(DisplayMode::Icon);
    match (display, item.icon.as_deref()) {
        (DisplayMode::Text, _) | (_, None) => 2.0 * m.toolbar_button.x,
        (DisplayMode::Icon, Some(_)) => m.toolbar_button.x,
        (DisplayMode::IconText, Some(_)) => {
            let label = item.label.clone().unwrap_or_default();
            let font = egui::TextStyle::Button.resolve(ui.style());
            let text = ui
                .painter()
                .layout_no_wrap(label, font, egui::Color32::WHITE)
                .rect
                .width();
            (m.icon_small + ui.spacing().icon_spacing + text + 2.0 * ui.spacing().button_padding.x)
                .max(m.toolbar_button.x)
        }
    }
}

/// The 7 px gripper at the start of every toolbar.
fn draw_gripper(ui: &mut Ui, id: &str) {
    let m = ClassicMetrics::for_ctx(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(m.toolbar_gripper_width, m.toolbar_button.y),
        Sense::hover(),
    );
    let painter = ui.painter();
    let color = crate::theme::color_at(ui.ctx(), "chrome.separator");
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
        let m = ClassicMetrics::for_ctx(ui.ctx());
        let margin = m.toolbar_separator_margin;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(m.toolbar_separator_width + 2.0 * margin, m.toolbar_button.y),
            Sense::hover(),
        );
        let y = rect.center().y;
        ui.painter().line_segment(
            [
                Pos2::new(rect.center().x, y - 8.0),
                Pos2::new(rect.center().x, y + 8.0),
            ],
            egui::Stroke::new(
                m.toolbar_separator_width,
                crate::theme::color_at(ui.ctx(), "chrome.separator"),
            ),
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
    let m = ClassicMetrics::for_ctx(ui.ctx());
    let response = match button_content(ui.ctx(), item, &label, checked, enabled) {
        Some(button) => {
            ui.add_enabled_ui(enabled, |ui| ui.add_sized(m.toolbar_button, button))
                .inner
        }
        None => {
            let button = egui::Button::new(label.clone()).selected(checked);
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    Vec2::new(2.0 * m.toolbar_button.x, m.toolbar_button.y),
                    button,
                )
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
fn button_content(
    ctx: &egui::Context,
    item: &Item,
    label: &str,
    checked: bool,
    enabled: bool,
) -> Option<egui::Button<'static>> {
    let icon_size = ClassicMetrics::for_ctx(ctx).icon_small;
    let display = item.display.unwrap_or(DisplayMode::Icon);
    match (display, item.icon.as_deref()) {
        (DisplayMode::Text, _) | (_, None) => None,
        (DisplayMode::Icon, Some(icon)) => Some(
            egui::Button::new(icons::icon_themed(ctx, icon, icon_size, enabled)).selected(checked),
        ),
        (DisplayMode::IconText, Some(icon)) => Some(
            egui::Button::new((
                icons::icon_themed(ctx, icon, icon_size, enabled),
                label.to_owned(),
            ))
            .selected(checked),
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
    let m = ClassicMetrics::for_ctx(ui.ctx());
    let response = match button_content(ui.ctx(), &main_item, &main_label, main_checked, enabled) {
        Some(button) => {
            ui.add_enabled_ui(enabled, |ui| ui.add_sized(m.toolbar_button, button))
                .inner
        }
        None => {
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    Vec2::new(2.0 * m.toolbar_button.x, m.toolbar_button.y),
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
    // The arrow part keeps its 11 px: no padding, a small arrow
    // (task P7-17: the 16 px arrow with button padding drew 30 px wide).
    let arrow_size = Vec2::new(m.toolbar_split_arrow_width, m.toolbar_button.y);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = Vec2::ZERO;
            let arrow = egui::Button::new(icons::dropdown_arrow(
                (m.toolbar_split_arrow_width - 2.0).max(4.0),
            ))
            .min_size(arrow_size);
            ui.add_sized(arrow_size, arrow)
        })
        .inner;
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
    let m = ClassicMetrics::for_ctx(ui.ctx());
    let content = egui::Button::new((
        icons::icon_themed(
            ui.ctx(),
            item.icon.as_deref().unwrap_or(&item.id),
            m.icon_small,
            enabled,
        ),
        icons::dropdown_arrow((m.toolbar_split_arrow_width - 2.0).max(4.0)),
    ));
    let response = ui.add_enabled_ui(enabled, |ui| {
        ui.spacing_mut().button_padding = Vec2::new(1.0, 0.0);
        ui.spacing_mut().icon_spacing = 0.0;
        ui.add_sized(
            Vec2::new(
                m.toolbar_button.x + m.toolbar_split_arrow_width,
                m.toolbar_button.y,
            ),
            content,
        )
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
/// with the `value` argument, reverts on Esc, and its ▾ button opens
/// the drop-down list (task P7-24 goals 2 and 3: the font and size
/// boxes get the pickers' lists and the zoom box the presets).
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
    let m = ClassicMetrics::for_ctx(ui.ctx());
    let width = item
        .width
        .map(|width| width as f32)
        .unwrap_or_else(|| combo_width(&item.id, &m));
    // The drop-down button sits inside the box's width (Visio's
    // combos are one box with an attached ▾ strip).
    let list = combo_list(item);
    let arrow_width = if list {
        m.toolbar_split_arrow_width
    } else {
        0.0
    };
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                Vec2::new(width - arrow_width, m.toolbar_button.y),
                egui::TextEdit::singleline(&mut buffer).hint_text(label.clone()),
            )
        })
        .inner;
    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::ComboBox, enabled, label.clone())
    });
    if list {
        let arrow = ui
            .scope(|ui| {
                ui.spacing_mut().button_padding = Vec2::ZERO;
                let arrow = egui::Button::new(icons::dropdown_arrow(
                    (m.toolbar_split_arrow_width - 2.0).max(4.0),
                ))
                .min_size(Vec2::new(m.toolbar_split_arrow_width, m.toolbar_button.y));
                ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        Vec2::new(m.toolbar_split_arrow_width, m.toolbar_button.y),
                        arrow,
                    )
                })
            })
            .inner
            .inner;
        arrow.widget_info(|| {
            WidgetInfo::labeled(egui::accesskit::Role::Button, true, format!("{label} menu"))
        });
        if arrow.clicked() {
            toggle_popup(state, item, arrow.rect);
        }
    }
    state.combo_text.insert(item.id.clone(), buffer.clone());
    // Enter applies and Esc reverts (egui surrenders text focus on the
    // key press, so the standard `lost_focus` idiom reads the key).
    let lost = response.lost_focus();
    if lost && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        if let Activation::Intent(mut intent) =
            rows::combo_activation(item, input, Source::Ui, buffer.clone())
        {
            // The typed value also goes as `percent`, the number the
            // zoom box's `shape.draw.zoom` reads (task P7-24 goal 3).
            if let Some(number) = typed_number(&buffer) {
                intent
                    .invocation
                    .args
                    .set("percent", larrygram_commands::ArgValue::Number(number));
            }
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

/// Whether one combo's drop-down button draws (task P7-24: the font
/// and size boxes list the pickers' rows and the zoom box the
/// presets).
fn combo_list(item: &Item) -> bool {
    item.command
        .as_ref()
        .map(|id| id.as_str())
        .and_then(crate::pickers::picker_for)
        .is_some()
}

/// The number a combo's typed value stands for: the text with an
/// optional `pt` or `%` suffix trimmed ("18 pt", "150%").
fn typed_number(typed: &str) -> Option<f64> {
    let trimmed = typed.trim();
    let trimmed = trimmed.strip_suffix('%').unwrap_or(trimmed).trim();
    let trimmed = trimmed
        .strip_suffix("pt")
        .or_else(|| trimmed.strip_suffix("PT"))
        .unwrap_or(trimmed)
        .trim();
    trimmed.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The themed width of one toolbar combo (`combo.font.width`,
/// `combo.font_size.width` and `combo.zoom.width`): the item id's last
/// segment names the combo.
fn combo_width(id: &str, m: &ClassicMetrics) -> f32 {
    match id.rsplit('.').next() {
        Some("font") => m.combo_font_width,
        Some("zoom") => m.combo_zoom_width,
        _ => m.combo_font_size_width,
    }
}

/// Opens or closes the popup of a toolbar item.
fn toggle_popup(state: &mut PanelClassic, item: &Item, rect: Rect) {
    if state.open_popup.as_deref() == Some(item.id.as_str()) {
        state.close_popup();
    } else {
        state.close_popup();
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
        state.close_popup();
        return;
    }
    let mut clicked: Option<(String, Activation)> = None;
    if open == TOOLBAR_LIST {
        let rows = toolbar_list_rows(input, state);
        clicked = draw_popup_levels(ui, &rows, state.popup_rect.left_bottom(), state);
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
            // A formatting picker hosts the popup (task P7-21): the
            // colour swatches, the line lists and the font lists draw
            // instead of the plain rows.
            let picker = item
                .command
                .as_ref()
                .map(|id| id.as_str())
                .and_then(crate::pickers::picker_for);
            if let Some(kind) = picker {
                let mut chosen = false;
                let area =
                    egui::Area::new(egui::Id::unique(("panel_classic_picker", item.id.as_str())))
                        .order(egui::Order::Foreground)
                        .fixed_pos(state.popup_rect.left_bottom())
                        .show(ui.ctx(), |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                chosen = crate::pickers::show(
                                    ui,
                                    kind,
                                    Source::Ui,
                                    crate::pickers::fill_preview(input.app),
                                    intents,
                                );
                            });
                        });
                state.popup_level_rects = vec![area.response.rect];
                if chosen {
                    state.close_popup();
                }
            } else {
                let rows = rows::rows(&item.items, input, state, Source::Ui, false, 0);
                clicked = draw_popup_levels(ui, &rows, state.popup_rect.left_bottom(), state);
            }
            // `split_recent`: the pick becomes the main button.
            if let Some((row_id, _)) = &clicked
                && item.kind == ItemKind::SplitRecent
            {
                state.split_recent.insert(item.id.clone(), row_id.clone());
            }
        } else {
            state.close_popup();
        }
    }
    if let Some((_, activation)) = clicked {
        match activation {
            Activation::Intent(intent) => {
                intents.push(intent);
                state.close_popup();
            }
            Activation::ToolbarToggle { toolbar, intent } => {
                if let Some(intent) = intent {
                    intents.push(intent);
                }
                let visible = state.toolbar_visible_in(input.layout, input.mode, &toolbar);
                state.set_toolbar_visible_in(input.mode, &toolbar, !visible);
                state.close_popup();
            }
            _ => {}
        }
    }
    if ui.input(|i| i.pointer.any_click())
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
        && !state.popup_hit_rect().contains(pointer)
        && !state.popup_rect.contains(pointer)
    {
        state.close_popup();
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

/// The open popup's levels and their chain state; returns the id and
/// the activation of the row the user clicked.
///
/// The chain is state (`PanelClassic::popup_chain`), so a submenu
/// stays open while the pointer moves into it — the `context_chain`
/// rule of the context menus (P7-12 major 4; the old stateless hover
/// closed a child level the moment the pointer left the parent row).
fn draw_popup_levels(
    ui: &mut Ui,
    rows: &[Row],
    pos: Pos2,
    state: &mut PanelClassic,
) -> Option<(String, Activation)> {
    let mut levels = vec![rows.to_vec()];
    for link in state.popup_chain.iter().copied().take(MAX_DEPTH as usize) {
        let Some(row) = levels
            .last()
            .and_then(|level| level.get(link))
            .filter(|row| row.has_children())
        else {
            break;
        };
        levels.push(row.children.clone());
    }
    let mut pos = pos;
    let mut activated: Option<(usize, usize)> = None;
    let mut open_child: Option<(usize, usize)> = None;
    let mut hover: Option<(usize, usize)> = None;
    let mut rects = Vec::with_capacity(levels.len());
    for (level, level_rows) in levels.iter().enumerate() {
        let width = menu::menu_width(ui, level_rows);
        let mut outcome = MenuOutcome::default();
        let area = egui::Area::new(Id::unique(("panel_classic_popup", level)))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    outcome = menu::draw_menu(ui, level_rows, width, None, false);
                });
            });
        rects.push(area.response.rect);
        if let Some(index) = outcome.clicked {
            if level_rows.get(index).is_some_and(Row::has_children) {
                open_child = Some((level, index));
            } else {
                activated = Some((level, index));
            }
        }
        if let Some(index) = outcome.hovered {
            hover = Some((level, index));
        }
        // The next level draws beside the open submenu row.
        if let Some(link) = state.popup_chain.get(level).copied()
            && let Some(rect) = outcome.rects.get(link).copied()
        {
            pos = rect.right_top();
        }
    }
    state.popup_level_rects = rects.clone();
    super::update_chain_hover(
        ui,
        &levels,
        &rects,
        hover,
        &mut state.popup_chain,
        &mut state.popup_hover,
        &mut state.popup_left,
    );
    if let Some((level, index)) = open_child {
        state.popup_chain.truncate(level);
        state.popup_chain.push(index);
        state.popup_hover = None;
    }
    activated.and_then(|(level, index)| {
        levels
            .get(level)
            .and_then(|level| level.get(index))
            .map(|row| (row.id.clone(), row.activation.clone()))
    })
}

/// The items of a menu tree, searched by placement id.
fn find_in_menus<'a>(menus: &'a [larrygram_ui_spec::v2::Menu], id: &str) -> Option<&'a Item> {
    menus
        .iter()
        .find_map(|menu| super::find_item(&menu.items, id))
}
