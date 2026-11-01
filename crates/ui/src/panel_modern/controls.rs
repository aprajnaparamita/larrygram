//! The control widgets of the Panel Modern command bar
//! (`docs/ui/panel-modern.md`, ADR 0061).
//!
//! One entry point per surface: [`show_control`] draws one control at
//! the rectangle [`super::place`] gave it, [`show_menu`] draws the
//! entries of a dropdown, split or gallery menu. Every activation
//! becomes an [`Intent`]; nothing executes a command itself (ADR 0028).

use egui::{
    Align2, Atom, Atoms, Button, ContainerAtom, Direction, Popup, Rect, RichText, TextWrapMode, Ui,
    Vec2,
};
use larrygram_commands::AppCtx;
use larrygram_ui_spec::v2::{ControlSize, Item, ItemKind};

use super::common::{
    Action, Arrow, DROPDOWN_TIP, MAX_DEPTH, Resolve, Toggles, arrow_atom, display_label, icon_key,
    spacer, too_deep, tooltip_text, with_value,
};
use super::metrics::Metrics;
use super::place::Placed;
use super::{ListData, ListEntries};
use crate::{Intent, UiError};

/// [`super::common::finish`], with the disabled reason also mapped to
/// the widget's accessible description: egui's `on_disabled_hover_text`
/// shows the tooltip but does not describe the widget, and the tests
/// read the reason from the description (task P7-24 goal 1).
fn finish(response: egui::Response, action: &Action, tip: &str, out: &mut Vec<Intent>) {
    match action {
        Action::Disabled { reason } => {
            response
                .on_hover_text(reason.as_str())
                .on_disabled_hover_text(reason.as_str());
        }
        _ => super::common::finish(response, action, tip, out),
    }
}

/// The gutter width of this frame (the shared `menu.gutter.width`
/// token: icons sit in a gutter column, `docs/ui/panel-modern.md`).
fn gutter(ctx: &egui::Context) -> f32 {
    crate::panel_classic::metrics::ClassicMetrics::for_ctx(ctx).menu_gutter_width
}

/// What the controls draw from: run-time lists, sizes, toggle state
/// and the application state behind the dynamic labels.
pub(crate) struct Draw<'a> {
    /// The run-time list contents (`list` fields).
    pub lists: &'a ListData,
    /// Widget sizes.
    pub metrics: &'a Metrics,
    /// Application state (dynamic labels).
    pub app: &'a AppCtx,
    /// Display-only toggle state.
    pub toggles: &'a mut Toggles,
}

/// Draws one control at `placed.rect` (ADR 0061 widget mapping).
///
/// `depth` bounds menu nesting (AGENTS.md section 6).
pub(crate) fn show_control(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    if depth >= MAX_DEPTH {
        too_deep(ui, item);
        return Ok(());
    }
    match item.kind {
        ItemKind::Button | ItemKind::Toggle => button(ui, item, placed, draw, resolve, out),
        ItemKind::Check => check(ui, item, placed, draw, resolve, out),
        ItemKind::Combo => combo(ui, item, placed, draw, resolve, out, depth),
        ItemKind::Split | ItemKind::SplitRecent => {
            split(ui, item, placed, draw, resolve, out, depth)
        }
        ItemKind::Dropdown | ItemKind::Gallery => {
            dropdown(ui, item, placed, draw, resolve, out, depth)
        }
        ItemKind::Launcher => launcher(ui, item, placed, draw, resolve, out),
        ItemKind::Palette => palette(ui, item, placed, draw, resolve, out, depth),
        // Menu-only kinds render as plain labels when they appear as a
        // control; their entries still draw (no placement is dropped).
        ItemKind::Submenu | ItemKind::Heading | ItemKind::List | ItemKind::Options => {
            ui.label(item.label.clone().unwrap_or_default());
            for child in &item.items {
                show_control(ui, child, placed, draw, resolve, out, depth + 1)?;
            }
            Ok(())
        }
    }
}

/// A plain button at its `size`: icon over the label (large), icon
/// beside the label (medium), icon only (small).
fn button(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let atoms = button_atoms(
        ui.ctx(),
        item,
        &label,
        placed.large,
        false,
        enabled(&action),
        draw.metrics,
    );
    let widget = Button::new(atoms)
        .selected(latched(draw.toggles, item))
        .min_size(Vec2::new(placed.rect.width().min(MAX_W), 0.0));
    let response = put(ui, placed.rect, enabled(&action), widget, item, "main");
    let response = response.accessible_name(label.as_str());
    finish(response, &action, &tooltip_text(&label, item), out);
    Ok(())
}

/// A check box with its label.
fn check(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let mut checked = latched(draw.toggles, item);
    let response = ui
        .push_id((item.id.as_str(), "check"), |ui| {
            ui.add_enabled_ui(enabled(&action), |ui| {
                ui.put(
                    placed.rect,
                    egui::Checkbox::new(&mut checked, label.as_str()),
                )
            })
            .inner
        })
        .inner
        .accessible_name(label.as_str());
    if response.clicked() {
        set_latched(draw.toggles, item, checked);
    }
    finish(response, &action, &tooltip_text(&label, item), out);
    Ok(())
}

/// A group launcher: the small corner button of the caption row. Its
/// tooltip names the dialog (`docs/ui/panel-modern.md`).
fn launcher(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let response = fixed_button(
        ui,
        placed.rect,
        enabled(&action),
        item,
        "launcher",
        &label,
        Arrow::Right,
    );
    finish(response, &action, &tooltip_text(&label, item), out);
    Ok(())
}

/// A small button that takes exactly `rect` and draws one arrow: the
/// launcher in the caption row's corner and the arrow part of a
/// medium or small split (task P7-17: an egui button grows to its
/// padded content, which spilled over the caption text and over the
/// split's main part).
fn fixed_button(
    ui: &mut Ui,
    rect: Rect,
    enabled: bool,
    item: &Item,
    part: &'static str,
    label: &str,
    arrow: Arrow,
) -> egui::Response {
    let id = ui.make_persistent_id((item.id.as_str(), part));
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = ui.interact(rect, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let visuals = if enabled {
        ui.style().interact(&response)
    } else {
        &ui.visuals().widgets.noninteractive
    };
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        ui.painter()
            .rect_filled(rect.shrink(1.0), 2.0, visuals.weak_bg_fill);
    }
    let color = if enabled {
        visuals.fg_stroke.color
    } else {
        crate::theme::color_at(ui.ctx(), "chrome.text_disabled")
    };
    let half = (rect.width().min(rect.height()) * 0.2).clamp(1.0, 4.0);
    let center = rect.center();
    let points = match arrow {
        Arrow::Down => vec![
            egui::pos2(center.x - half, center.y - half * 0.6),
            egui::pos2(center.x + half, center.y - half * 0.6),
            egui::pos2(center.x, center.y + half * 0.8),
        ],
        Arrow::Left => vec![
            egui::pos2(center.x + half * 0.6, center.y - half),
            egui::pos2(center.x - half * 0.8, center.y),
            egui::pos2(center.x + half * 0.6, center.y + half),
        ],
        Arrow::Right => vec![
            egui::pos2(center.x - half * 0.6, center.y - half),
            egui::pos2(center.x + half * 0.8, center.y),
            egui::pos2(center.x - half * 0.6, center.y + half),
        ],
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
    response.accessible_name(label)
}

/// The synthetic dropdown item a collapsed group draws as (task
/// P7-17): the group's caption, the icon of its first control with
/// one, large, with ▾.
pub(crate) fn collapsed_group_item(group: &larrygram_ui_spec::v2::Group) -> Item {
    let mut item = group
        .controls
        .iter()
        .find(|item| !item.hidden && icon_key(item).is_some())
        .cloned()
        .unwrap_or_else(|| {
            group
                .controls
                .first()
                .cloned()
                .unwrap_or_else(|| empty_item(group))
        });
    item.id = format!("{}.collapsed", group.id);
    item.kind = ItemKind::Dropdown;
    item.size = Some(ControlSize::Large);
    item.label = Some(group.label.clone().unwrap_or_default());
    item.command = None;
    item.column = Some(0);
    item.row = Some(0);
    item.items = Vec::new();
    item.list = None;
    item.options = Vec::new();
    item.shortcut = None;
    item.dynamic_label = false;
    item.hidden = false;
    item
}

/// An item with no fields set but the group's id (a group with no
/// controls collapses to a caption-only button).
fn empty_item(group: &larrygram_ui_spec::v2::Group) -> Item {
    Item {
        id: group.id.clone(),
        kind: ItemKind::Dropdown,
        label: None,
        access: None,
        shortcut: None,
        command: None,
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

/// The button of a collapsed group, at `rect`: the group's caption
/// under its first icon, with ▾. The caller opens the group's popup
/// from the response.
pub(crate) fn collapsed_group_button(
    ui: &mut Ui,
    group: &larrygram_ui_spec::v2::Group,
    rect: Rect,
    m: &Metrics,
) -> egui::Response {
    let item = collapsed_group_item(group);
    let label = item.label.clone().unwrap_or_default();
    let atoms = button_atoms(ui.ctx(), &item, &label, true, true, true, m);
    let response = put(ui, rect, true, Button::new(atoms), &item, "collapsed");
    response
        .accessible_name(label.as_str())
        .on_hover_text(label.as_str())
}

/// A combo box: the label in a framed box with an arrow; the popup
/// lists the `options` (`docs/ui/panel-modern.md`).
fn combo(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let atoms = AtomsBuilder::new()
        .push(spacer(4.0))
        .text(&label)
        .push(Atom::grow())
        .push(arrow_atom(10.0, Arrow::Down))
        .push(spacer(4.0))
        .build();
    let widget = Button::new(atoms)
        .wrap_mode(egui::TextWrapMode::Truncate)
        .min_size(Vec2::new(placed.rect.width().min(MAX_W), 0.0));
    let response = put(ui, placed.rect, enabled(&action), widget, item, "combo");
    let response = response.accessible_name(label.as_str());
    finish(response.clone(), &action, &tooltip_text(&label, item), out);
    if let Some(kind) = item
        .command
        .as_ref()
        .map(|id| id.as_str())
        .and_then(crate::pickers::picker_for)
    {
        if let Some(inner) = Popup::menu(&response).show(|ui| {
            if crate::pickers::show(
                    ui,
                    kind,
                    larrygram_commands::Source::CommandBar,
                    crate::pickers::fill_preview(draw.app),
                    out,
                ) {
                ui.close();
            }
        }) {
            let _ = inner;
        }
        return Ok(());
    }
    if let Some(inner) = Popup::menu(&response)
        .show(|ui| -> Result<(), UiError> { show_options(ui, item, draw, resolve, out, depth + 1) })
    {
        inner.inner?;
    }
    Ok(())
}

/// A split button: the icon part runs `command`, the arrow part opens
/// the menu. Large splits divide horizontally (icon above, label and
/// ▾ below); medium and small splits divide vertically with a 12 px
/// arrow (`docs/ui/panel-modern.md`).
fn split(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let (main, arrow) = if placed.large {
        let arrow_height = draw.metrics.stack_row_height.min(placed.rect.height());
        (
            Rect::from_min_max(
                placed.rect.min,
                egui::pos2(placed.rect.max.x, placed.rect.max.y - arrow_height),
            ),
            Rect::from_min_max(
                egui::pos2(placed.rect.min.x, placed.rect.max.y - arrow_height),
                placed.rect.max,
            ),
        )
    } else {
        let arrow_width = draw.metrics.split_arrow_width.min(placed.rect.width());
        (
            Rect::from_min_max(
                placed.rect.min,
                egui::pos2(placed.rect.max.x - arrow_width, placed.rect.max.y),
            ),
            Rect::from_min_max(
                egui::pos2(placed.rect.max.x - arrow_width, placed.rect.min.y),
                placed.rect.max,
            ),
        )
    };

    // The main part runs the command. A large split shows only its
    // icon there: the label sits in the arrow part below (task P7-17:
    // both parts drew it, the upper copy clipped).
    let atoms = if placed.large {
        AtomsBuilder::new()
            .push(egui_ui_icon_or_gap(
                ui.ctx(),
                item,
                draw.metrics.large_icon,
                enabled(&action),
            ))
            .build()
    } else {
        button_atoms(
            ui.ctx(),
            item,
            &label,
            placed.large,
            false,
            enabled(&action),
            draw.metrics,
        )
    };
    let widget = Button::new(atoms);
    let response = put(ui, main, enabled(&action), widget, item, "main");
    let response = response.accessible_name(label.as_str());
    finish(response, &action, &tooltip_text(&label, item), out);

    // The arrow part opens the menu. A medium or small split's arrow
    // is a fixed 12 pt strip: an egui button grew to 30 pt over the
    // main part (task P7-17).
    let opens = enabled(&action) || menu_reachable(item, resolve, depth);
    let response = if placed.large {
        let arrow_atoms = AtomsBuilder::new()
            .text(&label)
            .push(spacer(2.0))
            .push(arrow_atom(10.0, Arrow::Down))
            .build();
        let widget = Button::new(arrow_atoms).small();
        put(ui, arrow, opens, widget, item, "arrow")
    } else {
        fixed_button(ui, arrow, opens, item, "arrow", DROPDOWN_TIP, Arrow::Down)
    };
    let response = response
        .accessible_name(DROPDOWN_TIP)
        .on_hover_text(DROPDOWN_TIP);
    if let Some(kind) = item
        .command
        .as_ref()
        .map(|id| id.as_str())
        .and_then(crate::pickers::picker_for)
    {
        // The picker hosts the popup body; the item's own rows draw
        // below it (task P7-24: the Line split's Weight, Dashes and
        // Arrows submenus open from the same menu, like Visio's).
        if let Some(inner) = Popup::menu(&response).show(|ui| -> Result<(), UiError> {
            if crate::pickers::show(
                    ui,
                    kind,
                    larrygram_commands::Source::CommandBar,
                    crate::pickers::fill_preview(draw.app),
                    out,
                ) {
                ui.close();
                return Ok(());
            }
            if !item.items.is_empty() {
                ui.separator();
                let inherited = inherited_resolve(item, resolve);
                show_menu(ui, &item.items, draw, &inherited, out, depth + 1)?;
            }
            Ok(())
        }) {
            inner.inner?;
        }
        return Ok(());
    }
    if let Some(inner) = Popup::menu(&response).show(|ui| -> Result<(), UiError> {
        let inherited = inherited_resolve(item, resolve);
        show_menu(ui, &item.items, draw, &inherited, out, depth + 1)
    }) {
        inner.inner?;
    }
    Ok(())
}

/// A dropdown button: the whole button opens the `items`, `list`
/// entries and `options` as a menu. Galleries show their `list` as a
/// grid of thumbnails (`docs/ui/panel-modern.md`).
fn dropdown(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    let label = display_label(item, draw.app);
    let action = resolve(item);
    let atoms = button_atoms(
        ui.ctx(),
        item,
        &label,
        placed.large,
        true,
        enabled(&action),
        draw.metrics,
    );
    let opens = enabled(&action) || menu_reachable(item, resolve, depth);
    let widget = Button::new(atoms).min_size(Vec2::new(placed.rect.width().min(MAX_W), 0.0));
    let response = put(ui, placed.rect, opens, widget, item, "main");
    let response = response.accessible_name(label.as_str());
    let response = if opens && !enabled(&action) {
        response.on_hover_text(tooltip_text(&label, item))
    } else {
        response
    };
    finish(response.clone(), &action, &tooltip_text(&label, item), out);
    if let Some(kind) = item
        .command
        .as_ref()
        .map(|id| id.as_str())
        .and_then(crate::pickers::picker_for)
    {
        if let Some(inner) = Popup::menu(&response).show(|ui| {
            if crate::pickers::show(
                    ui,
                    kind,
                    larrygram_commands::Source::CommandBar,
                    crate::pickers::fill_preview(draw.app),
                    out,
                ) {
                ui.close();
            }
        }) {
            let _ = inner;
        }
        return Ok(());
    }
    if let Some(inner) = Popup::menu(&response).show(|ui| -> Result<(), UiError> {
        if item.kind == ItemKind::Gallery {
            show_gallery(ui, item, draw, resolve, out, depth + 1)?;
        }
        let inherited = inherited_resolve(item, resolve);
        show_menu(ui, &item.items, draw, &inherited, out, depth + 1)
    }) {
        inner.inner?;
    }
    Ok(())
}

/// A toolbar palette (Panel Classic data adapted through the v1
/// adapter): rendered as a dropdown of its graphic choices.
fn palette(
    ui: &mut Ui,
    item: &Item,
    placed: Placed,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    dropdown(ui, item, placed, draw, resolve, out, depth)
}

/// Whether a dropdown's or split's menu holds anything that runs: a
/// child whose command is enabled, a list or options row that runs
/// the opener's command, or such a row in a submenu (bounded by
/// [`MAX_DEPTH`]).
///
/// A menu control whose own command is a stub still opens when its
/// menu is reachable (task P7-17: a stub opener disabled its whole
/// menu, so no submenu of the committed layout could open).
pub(crate) fn menu_reachable(item: &Item, resolve: Resolve<'_>, depth: u32) -> bool {
    if depth >= MAX_DEPTH {
        return false;
    }
    let inherited = inherited_resolve(item, resolve);
    item.items.iter().any(|child| match child.kind {
        ItemKind::Heading => false,
        ItemKind::Submenu => menu_reachable(child, resolve, depth + 1),
        _ => {
            matches!(inherited(child), Action::Emit(_))
                || (!child.items.is_empty() && menu_reachable(child, resolve, depth + 1))
        }
    })
}

/// The resolver for a popup's child rows: an `options` or `list` row
/// with no command of its own picks the opener's command, so a
/// dropdown's choice dispatches the dropdown's command with its
/// `value` argument (P7-14's dispatch-argument fix).
fn inherited_resolve<'a>(opener: &'a Item, resolve: Resolve<'a>) -> impl Fn(&Item) -> Action + 'a {
    move |child: &Item| match child.kind {
        ItemKind::Options | ItemKind::List if child.command.is_none() => resolve(opener),
        _ => resolve(child),
    }
}

/// The entries of a dropdown, split or gallery menu.
pub(crate) fn show_menu(
    ui: &mut Ui,
    items: &[Item],
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    for (index, item) in items.iter().enumerate() {
        ui.push_id(index, |ui| {
            show_menu_entry(ui, item, draw, resolve, out, depth)
        })
        .inner?;
    }
    Ok(())
}

/// How long the pointer may stay outside the menus before an open
/// submenu closes (the Panel Classic menus' delay).
const SUBMENU_LEAVE_DELAY: f64 = 0.4;

/// The pointer-leave rule of the submenus (task P7-17, the Panel
/// Classic rule): once the pointer has moved off every menu, the
/// submenu button and the submenu itself and stayed off them for
/// [`SUBMENU_LEAVE_DELAY`], the open submenu closes. egui closes a
/// submenu only when another row of its parent is hovered.
///
/// "Off every menu" means over no foreground layer: each menu level is
/// its own popup layer, so a pointer resting in a deeper level keeps
/// the whole chain open. Only a pointer move starts the timer, so a
/// submenu the keyboard opened stays open under a resting pointer.
/// The top-level menu stays open until a click outside or Esc.
fn close_submenu_on_leave(ui: &Ui, button: &egui::Response, submenu: &egui::Response) {
    let ctx = ui.ctx();
    let key = submenu.id.with("larrygram_submenu_left");
    let (pos, moved, now) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.delta() != Vec2::ZERO,
            i.time,
        )
    });
    let inside = pos.is_some_and(|pos| {
        button.rect.contains(pos)
            || submenu.rect.contains(pos)
            || ctx
                .layer_id_at(pos)
                .is_some_and(|layer| layer.order == egui::Order::Foreground)
    });
    let left: Option<f64> = ctx.data(|data| data.get_temp(key));
    if inside {
        if left.is_some() {
            ctx.data_mut(|data| data.remove::<f64>(key));
        }
        return;
    }
    match left {
        Some(start) if now - start >= SUBMENU_LEAVE_DELAY => {
            ctx.data_mut(|data| data.remove::<f64>(key));
            egui::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
        }
        Some(start) => {
            let wait = (SUBMENU_LEAVE_DELAY - (now - start)).max(0.0);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        None if moved => {
            ctx.data_mut(|data| data.insert_temp(key, now));
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(SUBMENU_LEAVE_DELAY));
        }
        None => {}
    }
}

/// One menu entry (ADR 0061: the 24 px icon gutter, `options` as text
/// rows, `list` rows, `submenu` with ▸).
fn show_menu_entry(
    ui: &mut Ui,
    item: &Item,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    if depth >= MAX_DEPTH {
        too_deep(ui, item);
        return Ok(());
    }
    let label = display_label(item, draw.app);
    match item.kind {
        ItemKind::Heading => {
            ui.label(RichText::new(label).strong());
            Ok(())
        }
        ItemKind::List => {
            // A formatting picker hosts the row body when the list's
            // command names one (task P7-24: the Line split's Weight,
            // Dashes and Arrows lists are empty run-time lists; the
            // pickers' own rows are the choices).
            if let Some(kind) = item
                .command
                .as_ref()
                .map(|id| id.as_str())
                .and_then(crate::pickers::picker_for)
            {
                if crate::pickers::show(
                    ui,
                    kind,
                    larrygram_commands::Source::CommandBar,
                    crate::pickers::fill_preview(draw.app),
                    out,
                ) {
                    egui::menu::MenuState::from_ui(ui, |state, _| state.open_item = None);
                }
                return Ok(());
            }
            show_list_rows(ui, item, draw, resolve, out)
        }
        ItemKind::Options => show_options(ui, item, draw, resolve, out, depth),
        ItemKind::Submenu
        | ItemKind::Dropdown
        | ItemKind::Gallery
        | ItemKind::Split
        | ItemKind::SplitRecent
            if !item.items.is_empty() =>
        {
            let atoms = menu_row_atoms(ui.ctx(), item, &label, true);
            let button = Button::new(atoms).frame(false);
            // A submenu row opens its children on hover or click
            // (`SubMenuButton`), like the Panel Modern menus do.
            let (response, popup) = egui::menu::SubMenuButton::from_button(button).ui(
                ui,
                |ui| -> Result<(), UiError> {
                    show_menu(ui, &item.items, draw, resolve, out, depth + 1)
                },
            );
            if let Some(popup) = popup {
                close_submenu_on_leave(ui, &response, &popup.response);
                popup.inner?;
            }
            Ok(())
        }
        _ => {
            let action = resolve(item);
            let atoms = menu_row_atoms(ui.ctx(), item, &label, enabled(&action));
            let widget = Button::new(atoms).frame(false);
            let response = ui
                .add_enabled_ui(enabled(&action), |ui| ui.add(widget))
                .inner
                .accessible_name(label.as_str());
            let tip = match item.kind {
                ItemKind::Check | ItemKind::Toggle => {
                    let mark = if latched(draw.toggles, item) {
                        "on, "
                    } else {
                        "off, "
                    };
                    format!("{mark}{}", tooltip_text(&label, item))
                }
                _ => tooltip_text(&label, item),
            };
            // A disabled row's tooltip and description are the
            // reason, not the label (goal 1).
            let shown = match &action {
                Action::Disabled { reason } => reason.as_str(),
                Action::Emit(_) => tip.as_str(),
            };
            let response = response.on_hover_text(shown);
            if response.clicked() {
                if matches!(item.kind, ItemKind::Check | ItemKind::Toggle) {
                    set_latched(draw.toggles, item, !latched(draw.toggles, item));
                }
                if let Action::Emit(intent) = &action {
                    out.push(intent.clone());
                }
            }
            if let Action::Disabled { reason } = &action {
                response.on_disabled_hover_text(reason.as_str());
            }
            Ok(())
        }
    }
}

/// The `list` entries of a menu item, as rows. An empty list shows one
/// disabled "(none)" entry (`docs/ui/layout-format.md`).
fn show_list_rows(
    ui: &mut Ui,
    item: &Item,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
) -> Result<(), UiError> {
    let name = item.list.clone().unwrap_or_default();
    for entry in entry_rows(draw.lists, &name) {
        let action = with_value(&resolve(item), entry.text.as_str());
        let widget = Button::new(entry.text.as_str()).frame(false);
        let response = ui
            .add_enabled_ui(enabled(&action) && !entry.placeholder, |ui| ui.add(widget))
            .inner
            .accessible_name(entry.text.as_str());
        if entry.placeholder {
            response.on_disabled_hover_text("(none)");
        } else {
            finish(response, &action, entry.text.as_str(), out);
        }
    }
    Ok(())
}

/// The fixed `options` of a menu item or combo, as rows. An item with
/// no `options` lists its `items` instead.
fn show_options(
    ui: &mut Ui,
    item: &Item,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    if item.options.is_empty() {
        return show_menu(ui, &item.items, draw, resolve, out, depth);
    }
    for option in &item.options {
        let action = with_value(&resolve(item), option);
        let widget = Button::new(option.as_str()).frame(false);
        let response = ui
            .add_enabled_ui(enabled(&action), |ui| ui.add(widget))
            .inner
            .accessible_name(option.as_str());
        finish(response, &action, option, out);
    }
    Ok(())
}

/// A gallery's grid of `list` thumbnails, with a "(none)" placeholder
/// when the list is empty (`docs/ui/panel-modern.md`).
fn show_gallery(
    ui: &mut Ui,
    item: &Item,
    draw: &mut Draw<'_>,
    resolve: Resolve<'_>,
    out: &mut Vec<Intent>,
    depth: u32,
) -> Result<(), UiError> {
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    let name = item.list.clone().unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for entry in entry_rows(draw.lists, &name) {
            let action = with_value(&resolve(item), entry.text.as_str());
            let widget = Button::new(entry.text.as_str()).min_size(Vec2::new(64.0, 24.0));
            let response = ui
                .add_enabled_ui(enabled(&action) && !entry.placeholder, |ui| ui.add(widget))
                .inner
                .accessible_name(entry.text.as_str());
            if entry.placeholder {
                response.on_disabled_hover_text("(none)");
            } else {
                finish(response, &action, entry.text.as_str(), out);
            }
        }
        Ok(())
    })
    .inner
}

/// One run-time list entry: the text it draws and whether it is the
/// "(none)" placeholder.
struct EntryRow {
    /// The row text.
    text: String,
    /// True for the disabled "(none)" placeholder.
    placeholder: bool,
}

/// The rows of one run-time list; empty lists yield the placeholder.
fn entry_rows(lists: &ListData, name: &str) -> Vec<EntryRow> {
    match lists.entries(name) {
        ListEntries::Empty => vec![EntryRow {
            text: "(none)".to_owned(),
            placeholder: true,
        }],
        ListEntries::Some(entries) => entries
            .iter()
            .map(|text| EntryRow {
                text: text.clone(),
                placeholder: false,
            })
            .collect(),
    }
}

/// The largest width a widget is stretched to (a malformed `width`
/// can never take the whole screen).
const MAX_W: f32 = 2000.0;

/// The atoms of a command-bar button: icon and label at the control's
/// `size`, with a ▾ after the label for dropdowns.
fn button_atoms(
    ctx: &egui::Context,
    item: &Item,
    label: &str,
    large: bool,
    arrow: bool,
    enabled: bool,
    m: &Metrics,
) -> Atoms<'static> {
    let mut builder = AtomsBuilder::new().push(spacer(2.0));
    if large {
        // Icon on top, label (and ▾) below.
        let mut below = AtomsBuilder::new().text(label);
        if arrow {
            below = below.push(spacer(2.0)).push(arrow_atom(10.0, Arrow::Down));
        }
        let below = below.build();
        builder = builder.push(Atom::container(
            ContainerAtom::new((
                egui_ui_icon_or_gap(ctx, item, m.large_icon, enabled),
                Atom::container(ContainerAtom::new(below).direction(Direction::LeftToRight)),
            ))
            .direction(Direction::TopDown)
            .wrap_mode(TextWrapMode::Wrap)
            .align2(Align2::CENTER_CENTER),
        ));
    } else if item.size == Some(ControlSize::Small) {
        builder = builder.push(egui_ui_icon_or_gap(ctx, item, m.small_icon, enabled));
        if arrow {
            builder = builder
                .push(spacer(2.0))
                .push(arrow_atom(10.0, Arrow::Down));
        }
    } else {
        if icon_key(item).is_some() {
            builder = builder
                .push(egui_ui_icon(ctx, item, m.medium_icon, enabled))
                .push(spacer(4.0));
        }
        builder = builder.text(label);
        if arrow {
            builder = builder
                .push(spacer(2.0))
                .push(arrow_atom(10.0, Arrow::Down));
        }
    }
    builder.push(spacer(2.0)).build()
}

/// The atoms of a menu row: the 24 px icon gutter, the label and the
/// right-aligned shortcut text.
fn menu_row_atoms(ctx: &egui::Context, item: &Item, label: &str, enabled: bool) -> Atoms<'static> {
    let mut builder = AtomsBuilder::new().push(spacer(2.0));
    if item.icon.is_some() {
        builder = builder
            .push(egui_ui_icon(ctx, item, 16.0, enabled))
            .push(spacer(6.0));
    } else {
        builder = builder.push(spacer(gutter(ctx) - 4.0));
    }
    builder = builder.text(label).push(Atom::grow());
    if let Some(shortcut) = &item.shortcut {
        builder = builder.push(spacer(12.0)).text(shortcut);
    }
    if matches!(item.kind, ItemKind::Submenu) {
        builder = builder
            .push(spacer(4.0))
            .push(arrow_atom(10.0, Arrow::Right));
    }
    builder.push(spacer(4.0)).build()
}

/// The icon atom of an item: the key into `ui-spec/icons.yaml` with
/// the theme tint (P7-04's icon atoms, wired by task P7-09).
fn egui_ui_icon(ctx: &egui::Context, item: &Item, size: f32, enabled: bool) -> Atom<'static> {
    let key = icon_key(item).unwrap_or(item.id.as_str()).to_owned();
    crate::icons::icon_themed(ctx, &key, size, enabled)
}

/// The icon atom of an item, or a gap when it has no icon.
fn egui_ui_icon_or_gap(
    ctx: &egui::Context,
    item: &Item,
    size: f32,
    enabled: bool,
) -> Atom<'static> {
    if icon_key(item).is_some() {
        egui_ui_icon(ctx, item, size, enabled)
    } else {
        spacer(size)
    }
}

/// A tiny builder for the atom rows of a button.
struct AtomsBuilder {
    /// The atoms collected so far.
    atoms: Atoms<'static>,
}

impl AtomsBuilder {
    /// An empty builder.
    fn new() -> Self {
        Self {
            atoms: Atoms::new(spacer(0.0)),
        }
    }

    /// Appends an atom.
    fn push(mut self, atom: Atom<'static>) -> Self {
        self.atoms.push_right(atom);
        self
    }

    /// Appends a text atom.
    fn text(mut self, text: &str) -> Self {
        self.atoms.push_right(Atom::from(text.to_owned()));
        self
    }

    /// The collected atoms.
    fn build(self) -> Atoms<'static> {
        self.atoms
    }
}

/// Puts an enabled or disabled widget at `rect`, under an id scoped to
/// the item and its `part` (a split's two parts must not clash).
fn put(
    ui: &mut Ui,
    rect: Rect,
    enabled: bool,
    widget: impl egui::Widget,
    item: &Item,
    part: &'static str,
) -> egui::Response {
    ui.push_id((item.id.as_str(), part), |ui| {
        ui.add_enabled_ui(enabled, |ui| ui.put(rect, widget)).inner
    })
    .inner
}

/// Whether an action dispatches.
fn enabled(action: &Action) -> bool {
    matches!(action, Action::Emit(_))
}

/// Whether a `check` or `toggle` draws latched (display-only state).
fn latched(toggles: &Toggles, item: &Item) -> bool {
    toggles.contains(item.id.as_str())
}

/// Records the display-only latch of one control.
fn set_latched(toggles: &mut Toggles, item: &Item, latched: bool) {
    if latched {
        toggles.insert(item.id.clone());
    } else {
        toggles.remove(item.id.as_str());
    }
}

#[cfg(test)]
mod tests {
    use larrygram_commands::app::{AppCtx, Settings};

    use super::*;
    use crate::test_util::with_ui;

    /// A minimal item with a label and no command.
    fn item(id: &str, kind: ItemKind, label: &str) -> Item {
        Item {
            id: id.to_owned(),
            kind,
            label: Some(label.to_owned()),
            access: None,
            shortcut: None,
            command: None,
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

    /// A resolver that disables everything: the draw-only tests below
    /// render widgets but never click them.
    fn off(target: &Item) -> Action {
        Action::Disabled {
            reason: format!("no {:?}", target.id),
        }
    }

    /// The rectangle one control draws at.
    fn placed() -> Placed {
        Placed {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 32.0)),
            large: false,
        }
    }

    #[test]
    fn show_control_stops_at_the_recursion_cap() {
        with_ui(|ui| {
            let lists = ListData::new();
            let metrics = Metrics::default();
            let app = AppCtx::new(Settings::default());
            let mut toggles = Toggles::default();
            let mut draw = Draw {
                lists: &lists,
                metrics: &metrics,
                app: &app,
                toggles: &mut toggles,
            };
            let mut out = Vec::new();
            let target = item("deep", ItemKind::Button, "Too deep");
            let done = show_control(ui, &target, placed(), &mut draw, &off, &mut out, MAX_DEPTH);
            assert!(done.is_ok(), "the cap renders instead of erroring");
            assert!(out.is_empty(), "the cap dispatches nothing");
        });
    }

    #[test]
    fn menu_only_control_kinds_draw_labels_and_children() {
        with_ui(|ui| {
            let lists = ListData::new();
            let metrics = Metrics::default();
            let app = AppCtx::new(Settings::default());
            let mut toggles = Toggles::default();
            let mut draw = Draw {
                lists: &lists,
                metrics: &metrics,
                app: &app,
                toggles: &mut toggles,
            };
            let mut out = Vec::new();
            for kind in [
                ItemKind::Submenu,
                ItemKind::Heading,
                ItemKind::List,
                ItemKind::Options,
            ] {
                let mut parent = item("parent", kind, "Parent");
                parent.items = vec![item("child", ItemKind::Button, "Child")];
                let done = show_control(ui, &parent, placed(), &mut draw, &off, &mut out, 0);
                assert!(done.is_ok(), "{kind:?} renders as a label");
            }
        });
    }

    #[test]
    fn the_menu_seams_stop_at_the_recursion_cap() {
        with_ui(|ui| {
            let lists = ListData::new();
            let metrics = Metrics::default();
            let app = AppCtx::new(Settings::default());
            let mut toggles = Toggles::default();
            let mut draw = Draw {
                lists: &lists,
                metrics: &metrics,
                app: &app,
                toggles: &mut toggles,
            };
            let mut out = Vec::new();
            let target = item("entry", ItemKind::Button, "Entry");
            let items = vec![target.clone()];
            assert!(show_menu(ui, &items, &mut draw, &off, &mut out, MAX_DEPTH).is_ok());
            assert!(show_menu_entry(ui, &target, &mut draw, &off, &mut out, MAX_DEPTH).is_ok());
            assert!(show_options(ui, &target, &mut draw, &off, &mut out, MAX_DEPTH).is_ok());
            assert!(show_gallery(ui, &target, &mut draw, &off, &mut out, MAX_DEPTH).is_ok());
        });
    }

    #[test]
    fn entry_rows_maps_the_list_or_the_placeholder() {
        let mut lists = ListData::new();
        lists.insert("colors", vec!["Red".to_owned(), "Blue".to_owned()]);
        let rows = entry_rows(&lists, "colors");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "Red");
        assert!(!rows[0].placeholder);
        let none = entry_rows(&lists, "missing");
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].text, "(none)");
        assert!(none[0].placeholder);
    }

    #[test]
    fn latching_round_trips_through_the_toggle_set() {
        let mut toggles = Toggles::default();
        let target = item("check", ItemKind::Check, "Check");
        assert!(!latched(&toggles, &target));
        set_latched(&mut toggles, &target, true);
        assert!(latched(&toggles, &target));
        set_latched(&mut toggles, &target, false);
        assert!(!latched(&toggles, &target));
    }

    #[test]
    fn menu_rows_draw_every_entry_kind() {
        with_ui(|ui| {
            let mut lists = ListData::new();
            lists.insert("colors", vec!["Red".to_owned()]);
            let metrics = Metrics::default();
            let app = AppCtx::new(Settings::default());
            let mut toggles = Toggles::default();
            // The check entry starts latched, so both marks draw.
            toggles.insert("m.check".to_owned());
            let mut draw = Draw {
                lists: &lists,
                metrics: &metrics,
                app: &app,
                toggles: &mut toggles,
            };
            let mut rows: Vec<Item> = Vec::new();
            let mut heading = item("m.heading", ItemKind::Heading, "Section");
            heading.items = vec![item("m.heading.child", ItemKind::Button, "Ignored")];
            rows.push(heading);
            let mut list = item("m.list", ItemKind::List, "List");
            list.list = Some("colors".to_owned());
            rows.push(list);
            let mut empty_list = item("m.list.empty", ItemKind::List, "Empty");
            empty_list.list = Some("missing".to_owned());
            rows.push(empty_list);
            let mut options = item("m.options", ItemKind::Options, "Options");
            options.options = vec!["One".to_owned()];
            rows.push(options);
            let mut fallback = item("m.options.fallback", ItemKind::Options, "Fallback");
            fallback.items = vec![item("m.options.fallback.child", ItemKind::Button, "Child")];
            rows.push(fallback);
            let mut submenu = item("m.submenu", ItemKind::Submenu, "Submenu");
            submenu.icon = Some("edit.copy".to_owned());
            submenu.shortcut = Some("Ctrl+S".to_owned());
            submenu.items = vec![item("m.submenu.child", ItemKind::Button, "Deep")];
            rows.push(submenu);
            let mut check_entry = item("m.check", ItemKind::Check, "Check Entry");
            check_entry.shortcut = Some("Ctrl+K".to_owned());
            rows.push(check_entry);
            rows.push(item("m.toggle", ItemKind::Toggle, "Toggle Entry"));
            let mut plain = item("m.plain", ItemKind::Button, "Plain");
            plain.icon = Some("edit.paste".to_owned());
            rows.push(plain);
            rows.push(item("m.disabled", ItemKind::Button, "Disabled"));
            let mut out = Vec::new();
            assert!(show_menu(ui, &rows, &mut draw, &off, &mut out, 0).is_ok());
            assert!(out.is_empty(), "a draw-only frame dispatches nothing");
        });
    }

    #[test]
    fn gallery_and_list_rows_draw_their_entries() {
        with_ui(|ui| {
            let mut lists = ListData::new();
            lists.insert("colors", vec!["Red".to_owned(), "Blue".to_owned()]);
            let metrics = Metrics::default();
            let app = AppCtx::new(Settings::default());
            let mut toggles = Toggles::default();
            let mut draw = Draw {
                lists: &lists,
                metrics: &metrics,
                app: &app,
                toggles: &mut toggles,
            };
            let mut out = Vec::new();
            let mut gallery = item("g", ItemKind::Gallery, "Gallery");
            gallery.list = Some("colors".to_owned());
            assert!(show_gallery(ui, &gallery, &mut draw, &off, &mut out, 0).is_ok());
            let mut list = item("l", ItemKind::List, "List");
            list.list = Some("colors".to_owned());
            assert!(show_list_rows(ui, &list, &mut draw, &off, &mut out).is_ok());
        });
    }
}
