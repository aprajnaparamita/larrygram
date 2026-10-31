//! Menu painting for the Panel Classic widgets (`docs/ui/panel-classic.md`
//! "Menus"): the icon gutter, access-key underlines,
//! right-aligned shortcuts, separators and the submenu arrow.
//!
//! Colours come from the theme by token name (ADR 0063,
//! [`crate::theme::color_at`]) and the sizes from
//! [`ClassicMetrics`]; with no theme applied the defaults draw.

use egui::text::LayoutJob;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, WidgetInfo};

use crate::panel_classic::metrics::ClassicMetrics;
use crate::panel_classic::rows::{Activation, Row};

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

/// The widget sizes of this frame (the theme, or the defaults).
fn metrics(ui: &Ui) -> ClassicMetrics {
    ClassicMetrics::for_ctx(ui.ctx())
}

/// The width a menu of `rows` needs, in logical pixels.
pub(crate) fn menu_width(ui: &Ui, rows: &[Row]) -> f32 {
    let m = metrics(ui);
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
    (m.menu_gutter_width
        + m.menu_padding_x
        + label_width
        + m.menu_shortcut_gap
        + shortcut_width
        + m.menu_submenu_arrow_width
        + m.menu_padding_x)
        .max(120.0)
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
    let m = metrics(ui);
    let mut outcome = MenuOutcome::default();
    for (index, row) in rows.iter().enumerate() {
        if row.separator && index > 0 {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(width, m.menu_separator_height), Sense::hover());
            let y = rect.center().y;
            let color = crate::theme::color_at(ui.ctx(), "menu.separator");
            ui.painter().line_segment(
                [
                    Pos2::new(rect.left() + m.menu_gutter_width, y),
                    Pos2::new(rect.right() - m.menu_padding_x, y),
                ],
                Stroke::new(1.0, color),
            );
        }
        let response = draw_row(ui, row, width, highlight == Some(index), underlines, &m);
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
    m: &ClassicMetrics,
) -> egui::Response {
    let enabled = !matches!(row.activation, Activation::Disabled { .. });
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, m.menu_row_height), Sense::click());
    // The gutter column: the icon shelf at the left of the menu.
    let gutter = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top()),
        Vec2::new(m.menu_gutter_width, m.menu_row_height),
    );
    ui.painter()
        .rect_filled(gutter, 0.0, crate::theme::color_at(ui.ctx(), "menu.gutter"));
    if highlighted || response.hovered() {
        // The highlight sits inset from the menu edge with the control
        // radius (docs/ui/design.md).
        let radius = crate::theme::metric_at(ui.ctx(), "radius.control", 0.0);
        let rect = rect.shrink2(Vec2::new(2.0, 0.0));
        ui.painter().rect_filled(
            rect,
            radius,
            crate::theme::color_at(ui.ctx(), "menu.highlight.fill"),
        );
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0,
                crate::theme::color_at(ui.ctx(), "menu.highlight.border"),
            ),
            egui::StrokeKind::Inside,
        );
    }

    // The gutter: an icon, a check mark for a checked item with no
    // icon, or a pressed frame around a checked icon. The layout
    // switch rows draw a radio dot (task P7-23: `view.ui_layout.*`).
    match (&row.icon, row.check) {
        (Some(icon), checked) => {
            let icon_rect = Rect::from_center_size(gutter.center(), Vec2::splat(m.menu_icon));
            if checked == Some(true) {
                ui.painter().rect_stroke(
                    icon_rect.expand(2.0),
                    0.0,
                    Stroke::new(1.0, ui.visuals().widgets.active.bg_stroke.color),
                    egui::StrokeKind::Inside,
                );
            }
            paint_icon(ui, icon_rect, icon, enabled, m.menu_icon);
        }
        (None, Some(true)) if is_layout_radio(row) => {
            paint_radio(
                ui,
                gutter.center(),
                crate::theme::color_at(ui.ctx(), "chrome.text"),
            );
        }
        (None, Some(true)) => {
            paint_check(
                ui,
                gutter.center(),
                crate::theme::color_at(ui.ctx(), "chrome.text"),
            );
        }
        (None, _) => {}
    }

    // The label, with the access key underlined when the menu bar
    // shows its access keys.
    let text_color = if enabled {
        crate::theme::color_at(ui.ctx(), "chrome.text")
    } else {
        crate::theme::color_at(ui.ctx(), "chrome.text_disabled")
    };
    let font = text_font(ui);
    let job = label_job(&row.label, row.access, underlines, &font, text_color);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        Pos2::new(
            rect.left() + m.menu_gutter_width + m.menu_padding_x * 0.5,
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
                rect.right() - m.menu_padding_x - m.menu_submenu_arrow_width - galley.size().x,
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
                Pos2::new(
                    rect.right() - m.menu_padding_x - m.menu_submenu_arrow_width * 0.5,
                    rect.center().y,
                ),
                Vec2::splat(8.0),
            ),
            text_color,
        );
    }

    response.widget_info(|| {
        WidgetInfo::labeled(egui::accesskit::Role::MenuItem, enabled, row.label.clone())
    });
    if !row.tooltip.is_empty() {
        // The hint appears next to the item under the pointer (task
        // P7-23), not below the row's left edge.
        response = response.on_hover_text_at_pointer(row.tooltip.clone());
    }
    response
}

/// Whether `row` is one of the layout switch rows (`view.ui_layout.*`
/// in a [`Row`]): the radio dot of the gutter (task P7-23, ADR 0032).
fn is_layout_radio(row: &Row) -> bool {
    match &row.activation {
        Activation::Intent(intent) => intent.command.as_str().starts_with("view.ui_layout."),
        _ => false,
    }
}

/// The menu-bar title ("File"): one menu bar high, with the open and
/// hover look (`menu.bar.open.*` and `button.hover.fill`).
pub(crate) fn draw_title(
    ui: &mut Ui,
    label: &str,
    access: Option<char>,
    open: bool,
    underlines: bool,
) -> egui::Response {
    let m = metrics(ui);
    let font = text_font(ui);
    let galley = ui
        .ctx()
        .fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE));
    let size = Vec2::new(
        galley.size().x + 2.0 * m.menu_bar_item_padding_x,
        m.menu_bar_height,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let radius = crate::theme::metric_at(ui.ctx(), "radius.control", 0.0);
    if open {
        ui.painter().rect_filled(
            rect,
            radius,
            crate::theme::color_at(ui.ctx(), "menu.bar.open.fill"),
        );
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(
                1.0,
                crate::theme::color_at(ui.ctx(), "menu.bar.open.border"),
            ),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        ui.painter().rect_filled(
            rect,
            radius,
            crate::theme::color_at(ui.ctx(), "button.hover.fill"),
        );
    }
    let text_color = crate::theme::color_at(ui.ctx(), "chrome.text");
    let job = label_job(label, access, underlines, &font, text_color);
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );
    response
        .widget_info(|| WidgetInfo::labeled(egui::accesskit::Role::Button, true, label.to_owned()));
    response
}

/// A label with the access-key letter underlined (`underlines` on),
/// painted in `color`.
///
/// The first character equal to the access key (case-insensitive)
/// gets a 1 px underline; the label is otherwise plain.
fn label_job(
    text: &str,
    access: Option<char>,
    underlines: bool,
    font: &FontId,
    color: Color32,
) -> LayoutJob {
    let plain = egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let underline = egui::TextFormat {
        underline: Stroke::new(1.0, color),
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

/// The font of the chrome text (the theme's `ui` role through egui).
fn text_font(ui: &Ui) -> FontId {
    egui::TextStyle::Button.resolve(ui.style())
}

/// Paints the icon of `key` into `rect` at the row's enabled state
/// (ADR 0062: a disabled control draws its icon at 40% alpha).
fn paint_icon(ui: &mut Ui, rect: Rect, key: &str, enabled: bool, icon_size: f32) {
    let cell = Rect::from_min_size(rect.min, Vec2::splat(icon_size));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(cell)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    if let Some(theme) = crate::theme::current(child.ctx()) {
        let _ = crate::icons::draw(&mut child, cell, key, icon_size, &theme, enabled);
    } else {
        let _ = crate::icons::placeholder(&mut child, key, icon_size);
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

/// The dot of a selected radio row (task P7-23).
fn paint_radio(ui: &Ui, center: Pos2, color: Color32) {
    ui.painter().circle_filled(center, 3.0, color);
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
