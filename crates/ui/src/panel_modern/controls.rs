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
    Action, Arrow, DROPDOWN_TIP, MAX_DEPTH, Resolve, Toggles, arrow_atom, display_label, finish,
    spacer, too_deep, tooltip_text,
};
use super::metrics::Metrics;
use super::place::Placed;
use super::{ListData, ListEntries};
use crate::{Intent, UiError};

/// The menu gutter: icons sit in a 24 px column (the Panel Classic
/// menu gutter, `docs/ui/panel-modern.md`).
const GUTTER: f32 = 24.0;

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
    let atoms = button_atoms(item, &label, placed.large, false, draw.metrics);
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
    let atoms = AtomsBuilder::new()
        .push(spacer(2.0))
        .push(arrow_atom(10.0, Arrow::Right))
        .push(spacer(2.0))
        .build();
    let widget = Button::new(atoms).small();
    let response = put(ui, placed.rect, enabled(&action), widget, item, "launcher");
    let response = response.accessible_name(label.as_str());
    finish(response, &action, &tooltip_text(&label, item), out);
    Ok(())
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
    let widget = Button::new(atoms).min_size(Vec2::new(placed.rect.width().min(MAX_W), 0.0));
    let response = put(ui, placed.rect, enabled(&action), widget, item, "combo");
    let response = response.accessible_name(label.as_str());
    finish(response.clone(), &action, &tooltip_text(&label, item), out);
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

    // The main part runs the command.
    let atoms = button_atoms(item, &label, placed.large, false, draw.metrics);
    let widget = Button::new(atoms);
    let response = put(ui, main, enabled(&action), widget, item, "main");
    let response = response.accessible_name(label.as_str());
    finish(response, &action, &tooltip_text(&label, item), out);

    // The arrow part opens the menu.
    let arrow_atoms = if placed.large {
        AtomsBuilder::new()
            .text(&label)
            .push(spacer(2.0))
            .push(arrow_atom(10.0, Arrow::Down))
            .build()
    } else {
        AtomsBuilder::new()
            .push(arrow_atom(10.0, Arrow::Down))
            .build()
    };
    let widget = Button::new(arrow_atoms).small();
    let response = put(ui, arrow, enabled(&action), widget, item, "arrow");
    let response = response
        .accessible_name(DROPDOWN_TIP)
        .on_hover_text(DROPDOWN_TIP);
    if let Some(inner) = Popup::menu(&response).show(|ui| -> Result<(), UiError> {
        show_menu(ui, &item.items, draw, resolve, out, depth + 1)
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
    let atoms = button_atoms(item, &label, placed.large, true, draw.metrics);
    let widget = Button::new(atoms).min_size(Vec2::new(placed.rect.width().min(MAX_W), 0.0));
    let response = put(ui, placed.rect, enabled(&action), widget, item, "main");
    let response = response.accessible_name(label.as_str());
    finish(response.clone(), &action, &tooltip_text(&label, item), out);
    if let Some(inner) = Popup::menu(&response).show(|ui| -> Result<(), UiError> {
        if item.kind == ItemKind::Gallery {
            show_gallery(ui, item, draw, resolve, out, depth + 1)?;
        }
        show_menu(ui, &item.items, draw, resolve, out, depth + 1)
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
        ItemKind::List => show_list_rows(ui, item, draw, resolve, out),
        ItemKind::Options => show_options(ui, item, draw, resolve, out, depth),
        ItemKind::Submenu
        | ItemKind::Dropdown
        | ItemKind::Gallery
        | ItemKind::Split
        | ItemKind::SplitRecent
            if !item.items.is_empty() =>
        {
            let atoms = menu_row_atoms(item, &label);
            let button = Button::new(atoms).frame(false);
            // A submenu row opens its children on hover or click
            // (`SubMenuButton`), like the Panel Modern menus do.
            let (_response, popup) = egui::menu::SubMenuButton::from_button(button).ui(
                ui,
                |ui| -> Result<(), UiError> {
                    show_menu(ui, &item.items, draw, resolve, out, depth + 1)
                },
            );
            if let Some(popup) = popup {
                popup.inner?;
            }
            Ok(())
        }
        _ => {
            let action = resolve(item);
            let atoms = menu_row_atoms(item, &label);
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
            let response = response.on_hover_text(tip);
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
        let action = resolve(item);
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
        let action = resolve(item);
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
            let action = resolve(item);
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
fn button_atoms(item: &Item, label: &str, large: bool, arrow: bool, m: &Metrics) -> Atoms<'static> {
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
                egui_ui_icon_or_gap(item, m.large_icon),
                Atom::container(ContainerAtom::new(below).direction(Direction::LeftToRight)),
            ))
            .direction(Direction::TopDown)
            .wrap_mode(TextWrapMode::Wrap)
            .align2(Align2::CENTER_CENTER),
        ));
    } else if item.size == Some(ControlSize::Small) {
        builder = builder.push(egui_ui_icon_or_gap(item, m.small_icon));
        if arrow {
            builder = builder
                .push(spacer(2.0))
                .push(arrow_atom(10.0, Arrow::Down));
        }
    } else {
        if item.icon.is_some() {
            builder = builder
                .push(egui_ui_icon(item, m.medium_icon))
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
fn menu_row_atoms(item: &Item, label: &str) -> Atoms<'static> {
    let mut builder = AtomsBuilder::new().push(spacer(2.0));
    if item.icon.is_some() {
        builder = builder.push(egui_ui_icon(item, 16.0)).push(spacer(6.0));
    } else {
        builder = builder.push(spacer(GUTTER - 4.0));
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

/// The icon atom of an item: the key into `ui-spec/icons.yaml`, today
/// the placeholder mark of ADR 0030 until P7-04's pipeline lands.
fn egui_ui_icon(item: &Item, size: f32) -> Atom<'static> {
    let key = item.icon.clone().unwrap_or_else(|| item.id.clone());
    crate::icons::icon(&key, size)
}

/// The icon atom of an item, or a gap when it has no icon.
fn egui_ui_icon_or_gap(item: &Item, size: f32) -> Atom<'static> {
    if item.icon.is_some() {
        egui_ui_icon(item, size)
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
