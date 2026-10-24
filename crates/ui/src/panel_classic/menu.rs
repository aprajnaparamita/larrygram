//! Menu painting for the Panel Classic widgets (`docs/ui/panel-classic.md`
//! "Menus"): the 24 px icon gutter, access-key underlines,
//! right-aligned shortcuts, separators and the submenu arrow.
//!
//! Colours come from the egui defaults until task P7-05 merges the
//! theme tokens; the metrics below are the layout's numbers.

use egui::text::LayoutJob;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo};

use crate::panel_classic::rows::{Activation, Row};

/// Height of one menu item, in logical pixels (`docs/ui/panel-classic.md`).
pub(crate) const ROW_HEIGHT: f32 = 23.0;

/// Width of the icon gutter.
pub(crate) const GUTTER: f32 = 24.0;

/// Menu icon size.
pub(crate) const ICON: f32 = 16.0;

/// Width reserved for the submenu arrow.
pub(crate) const ARROW_WIDTH: f32 = 16.0;

/// Side padding inside a menu.
const PAD: f32 = 6.0;

/// Height of the separator line block.
const SEPARATOR: f32 = 5.0;

/// What one drawn menu level reported.
#[derive(Debug, Default)]
pub(crate) struct MenuOutcome {
    /// The row index the pointer clicked (the caller activates it).
    pub clicked: Option<usize>,
    /// The row index the pointer hovers.
    pub hovered: Option<usize>,
    /// The drawn rect of every row (submenu positioning).
    pub rects: Vec<Rect>,
}

/// The width a menu of `rows` needs, in logical pixels.
pub(crate) fn menu_width(ui: &Ui, rows: &[Row]) -> f32 {
    let font = text_font(ui);
    let mut label_width = 0.0f32;
    let mut shortcut_width = 0.0f32;
    for row in rows {
        let label = ui
            .ctx()
            .fonts_mut(|f| f.layout_no_wrap(row.label.clone(), font.clone(), Color32::WHITE));
        label_width = label_width.max(label.size().x);
        if let Some(shortcut) = &row.shortcut {
            let text = ui
                .ctx()
                .fonts_mut(|f| f.layout_no_wrap(shortcut.clone(), font.clone(), Color32::WHITE));
            shortcut_width = shortcut_width.max(text.size().x);
        }
    }
    (GUTTER + PAD + label_width + 2.0 * PAD + shortcut_width + ARROW_WIDTH + PAD).max(120.0)
}

/// Draws one menu level: separator blocks and one row per [`Row`].
///
/// `highlight` is the keyboard-highlighted row, `underlines` shows the
/// access-key underlines (Alt or an open menu).
pub(crate) fn draw_menu(
    ui: &mut Ui,
    rows: &[Row],
    width: f32,
    highlight: Option<usize>,
    underlines: bool,
) -> MenuOutcome {
    let mut outcome = MenuOutcome::default();
    for (index, row) in rows.iter().enumerate() {
        if row.separator && index > 0 {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SEPARATOR), Sense::hover());
            let y = rect.center().y;
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left() + GUTTER, y),
                    Pos2::new(rect.right() - PAD, y),
                ],
                Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
        }
        let response = draw_row(ui, row, width, highlight == Some(index), underlines);
        outcome.rects.push(response.rect);
        if response.clicked() {
            outcome.clicked = Some(index);
        }
        if response.hovered() {
            outcome.hovered = Some(index);
        }
    }
    outcome
}

/// One menu row: gutter (icon or check mark), label, shortcut and
/// submenu arrow (the `docs/ui/panel-classic.md` rules).
fn draw_row(
    ui: &mut Ui,
    row: &Row,
    width: f32,
    highlighted: bool,
    underlines: bool,
) -> egui::Response {
    let enabled = !matches!(row.activation, Activation::Disabled { .. });
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
    if highlighted || response.hovered() {
        let visuals = ui.visuals();
        ui.painter()
            .rect_filled(rect, 0.0, visuals.selection.bg_fill);
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, visuals.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }

    // The gutter: an icon, a check mark for a checked item with no
    // icon, or a pressed frame around a checked icon.
    let gutter = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top()),
        Vec2::new(GUTTER, ROW_HEIGHT),
    );
    match (&row.icon, row.check) {
        (Some(icon), checked) => {
            let icon_rect = Rect::from_center_size(gutter.center(), Vec2::splat(ICON));
            if checked == Some(true) {
                ui.painter().rect_stroke(
                    icon_rect.expand(2.0),
                    0.0,
                    Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
            paint_icon(ui, icon_rect, icon, enabled);
        }
        (None, Some(true)) => {
            paint_check(ui, gutter.center(), ui.visuals().strong_text_color());
        }
        (None, _) => {}
    }

    // The label, with the access key underlined when the menu bar
    // shows its access keys.
    let font = text_font(ui);
    let job = label_job(&row.label, row.access, underlines, &font);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let text_color = if enabled {
        ui.visuals().text_color()
    } else {
        ui.visuals().widgets.noninteractive.fg_stroke.color
    };
    ui.painter().galley(
        Pos2::new(
            rect.left() + GUTTER + PAD * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );

    // The shortcut, right-aligned before the arrow slot.
    if let Some(shortcut) = &row.shortcut {
        let galley = ui
            .ctx()
            .fonts_mut(|f| f.layout_no_wrap(shortcut.clone(), font.clone(), text_color));
        ui.painter().galley(
            Pos2::new(
                rect.right() - PAD - ARROW_WIDTH - galley.size().x,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            text_color,
        );
    }

    if row.has_children() {
        paint_arrow_right(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.right() - PAD - ARROW_WIDTH * 0.5, rect.center().y),
                Vec2::splat(8.0),
            ),
            text_color,
        );
    }

    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::MenuItem, enabled, row.label.clone())
    });
    if !row.tooltip.is_empty() {
        response = response.on_hover_text(row.tooltip.clone());
    }
    response
}

/// The menu-bar title ("File"): 24 px high, with the open and hover
/// look (`menu.bar.open.fill` in the theme, egui defaults here).
pub(crate) fn draw_title(
    ui: &mut Ui,
    label: &str,
    access: Option<char>,
    open: bool,
    underlines: bool,
) -> egui::Response {
    let font = text_font(ui);
    let galley = ui
        .ctx()
        .fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE));
    let size = Vec2::new(galley.size().x + 2.0 * PAD, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    if open {
        ui.painter()
            .rect_filled(rect, 0.0, visuals.selection.bg_fill);
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, visuals.selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, visuals.widgets.hovered.bg_fill);
    }
    let job = label_job(label, access, underlines, &font);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        visuals.text_color(),
    );
    response
        .widget_info(|| WidgetInfo::labeled(egui::accesskit::Role::Button, true, label.to_owned()));
    response
}

/// A label with the access-key letter underlined (`underlines` on).
///
/// The first character equal to the access key (case-insensitive)
/// gets a 1 px underline; the label is otherwise plain.
fn label_job(text: &str, access: Option<char>, underlines: bool, font: &FontId) -> LayoutJob {
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color: Color32::WHITE,
        ..Default::default()
    };
    let underline = egui::TextFormat {
        underline: Stroke::new(1.0, Color32::WHITE),
        ..plain.clone()
    };
    let key = access.map(|access| access.to_ascii_lowercase());
    let mut job = LayoutJob::default();
    let mut underlined = false;
    for (index, character) in text.char_indices() {
        let is_key = underlines
            && !underlined
            && key.is_some_and(|key| character.to_ascii_lowercase() == key);
        job.append(
            &text[index..index + character.len_utf8()],
            0.0,
            if is_key {
                underlined = true;
                underline.clone()
            } else {
                plain.clone()
            },
        );
    }
    if text.is_empty() {
        job.append("", 0.0, plain);
    }
    job
}

/// The font of the chrome text (egui defaults until P7-05).
fn text_font(ui: &Ui) -> FontId {
    egui::TextStyle::Button.resolve(ui.style())
}

/// Paints the small icon mark of `key` into `rect` (the placeholder
/// mark of `crates/ui/src/icons`, ADR 0030).
fn paint_icon(ui: &mut Ui, rect: Rect, key: &str, enabled: bool) {
    let cell = Rect::from_min_size(rect.min, Vec2::splat(ICON));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cell)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if enabled {
        let _ = crate::icons::placeholder(&mut child, key, ICON);
    } else {
        let faded = child.visuals().widgets.noninteractive.fg_stroke.color;
        child.painter().rect_filled(rect, 0.0, faded);
    }
}

/// A check mark for a checked item with no icon.
fn paint_check(ui: &Ui, center: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    ui.painter().line_segment(
        [
            Pos2::new(center.x - 5.0, center.y),
            Pos2::new(center.x - 1.5, center.y + 3.5),
        ],
        stroke,
    );
    ui.painter().line_segment(
        [
            Pos2::new(center.x - 1.5, center.y + 3.5),
            Pos2::new(center.x + 5.5, center.y - 4.0),
        ],
        stroke,
    );
}

/// A right-pointing triangle (egui's fonts have no ▸ glyph).
pub(crate) fn paint_arrow_right(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(rect.center().x - rect.width() * 0.25, rect.top()),
            Pos2::new(rect.center().x + rect.width() * 0.25, rect.center().y),
            Pos2::new(rect.center().x - rect.width() * 0.25, rect.bottom()),
        ],
        color,
        Stroke::NONE,
    ));
}
