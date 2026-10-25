//! The welcome wizard (ADR 0066, `docs/phase-7/README.md`).
//!
//! The wizard opens on the first launch, before any document, and
//! again from **Help > Welcome**. It has two pages: Welcome, and
//! Choose your layout. Task P7-15 implements the two pages; the
//! caller (the shell, task P7-09) writes the settings and closes the
//! wizard after an [`Outcome`].

use egui::Ui;
use larrygram_ui_spec::v2::LayoutKind;

use crate::UiError;
use crate::about;

/// The title on the Welcome page (ADR 0066, verbatim).
const TITLE: &str = "Larrygram v1.0 alpha";

/// The short description on the Welcome page (ADR 0066, verbatim).
const DESCRIPTION: &str = "Larrygram is a free, open-source diagramming app. Draw flowcharts and diagrams with shapes and connectors, and open, edit and save .vsdx drawings, on the desktop or in your browser.";

/// The title of the About window opened by **Why Larrygram?**.
const ABOUT_TITLE: &str = "About Larrygram";

/// The wizard page shown in [`show`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Page 1: the picture, the title and the short description.
    Welcome,
    /// Page 2: the two layout buttons.
    ChooseLayout,
}

/// The transient state of one wizard run.
///
/// The persisted parts live on
/// [`Settings`](larrygram_commands::Settings): `welcome_seen` and the
/// default layout. [`show`] reads neither; the caller passes the
/// state in and applies the [`Outcome`] (task P7-09 writes both
/// fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    page: Page,
    default_layout: LayoutKind,
    /// Whether the About window opened from **Why Larrygram?** is open.
    about_open: bool,
}

impl State {
    /// A fresh wizard on the Welcome page with Panel Modern selected.
    pub fn new() -> Self {
        Self {
            page: Page::Welcome,
            default_layout: LayoutKind::PanelModern,
            about_open: false,
        }
    }

    /// The page shown now.
    pub fn page(&self) -> Page {
        self.page
    }

    /// The layout the wizard would start with.
    pub fn default_layout(&self) -> LayoutKind {
        self.default_layout
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// What the caller must do after a [`show`] frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Cancel on either page: close the wizard and keep the default
    /// layout. The wizard is marked as seen.
    Cancel,
    /// Start on page 2: close the wizard, make `default_layout` the
    /// default layout and apply it at once. The wizard is marked as
    /// seen.
    Start {
        /// The layout the user chose.
        default_layout: LayoutKind,
    },
}

/// Draws one frame of the welcome wizard.
///
/// Returns `Ok(None)` while the wizard is open, and an [`Outcome`]
/// when the user closes it. Next and Back move between the pages
/// inside `state`. **Why Larrygram?** opens the About window through
/// [`about::show`].
pub fn show(ui: &mut Ui, state: &mut State) -> Result<Option<Outcome>, UiError> {
    show_with_about(ui, state, about::show)
}

/// Draws one frame of the welcome wizard with an injectable About
/// window.
///
/// [`show`] passes [`about::show`]; tests pass a
/// fake so they can observe that **Why Larrygram?** opened the window
/// without depending on the About task's implementation.
pub fn show_with_about<F>(
    ui: &mut Ui,
    state: &mut State,
    about_window: F,
) -> Result<Option<Outcome>, UiError>
where
    F: FnMut(&mut Ui) -> Result<about::Outcome, UiError>,
{
    let outcome = match state.page {
        Page::Welcome => welcome_page(ui, state, about_window)?,
        Page::ChooseLayout => choose_layout_page(ui, state),
    };
    Ok(outcome)
}

/// The heading on the Choose your layout page.
const LAYOUT_HEADING: &str = "Choose your layout";

/// The share of the Welcome page's width the picture takes.
const ART_SHARE: f32 = 0.55;

/// Width over height of the brand art (`viewBox="0 0 1600 1000"`).
const ART_ASPECT: f32 = 1.6;

/// The body text size at a text scale of 1, in points.
const BODY_SIZE: f32 = 14.0;

/// The heading text size at a text scale of 1, in points.
const HEADING_SIZE: f32 = 22.0;

/// The height of a layout choice's name button.
const CHOICE_BUTTON_HEIGHT: f32 = 32.0;

/// The gap between a layout choice's picture and its name button.
const CHOICE_GAP: f32 = 8.0;

/// One footer button, sized with the window's text scale.
fn footer_button(ui: &mut Ui, label: &str, scale: f32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(BODY_SIZE * scale))
            .min_size(egui::vec2(80.0 * scale, 0.0)),
    )
}

/// The whole drawing rect of one wizard frame: everything the caller's
/// ui offers (the shell sizes its window with
/// [`window_size::WIZARD`](crate::window_size::WIZARD)).
fn frame_rect(ui: &mut Ui) -> egui::Rect {
    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, egui::Sense::hover());
    rect
}

/// A child ui over `rect` with `layout`, for one region of the page.
fn region(ui: &mut Ui, rect: egui::Rect, layout: egui::Layout) -> Ui {
    ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout))
}

/// Draws page 1 and returns its outcome.
///
/// The picture fills the left [`ART_SHARE`] of the body at its own
/// aspect ratio; the title, the description and **Why Larrygram?** sit
/// on the right; Cancel and Next sit in the footer along the bottom
/// edge. The text grows with the window (task P7-17).
fn welcome_page<F>(
    ui: &mut Ui,
    state: &mut State,
    mut about_window: F,
) -> Result<Option<Outcome>, UiError>
where
    F: FnMut(&mut Ui) -> Result<about::Outcome, UiError>,
{
    use crate::window_size::{GAP, fit_aspect, split_footer, text_scale};

    let mut outcome = None;
    let mut open_about = state.about_open;
    let mut close_about = false;

    let rect = frame_rect(ui);
    let (body, footer) = split_footer(rect);
    let scale = text_scale(rect.width());
    let art_box = egui::Rect::from_min_size(
        body.min,
        egui::vec2((body.width() * ART_SHARE).round(), body.height()),
    );
    let art = fit_aspect(art_box, ART_ASPECT);
    ui.put(
        art,
        egui::Image::new(egui::include_image!("../assets/brand/about_screen.svg"))
            .fit_to_exact_size(art.size())
            .alt_text("Larrygram welcome picture"),
    );
    let text = egui::Rect::from_min_max(egui::pos2(art_box.max.x + GAP, body.min.y), body.max);
    let mut column = region(ui, text, egui::Layout::top_down(egui::Align::Min));
    column.add_space(GAP);
    column.label(
        egui::RichText::new(TITLE)
            .size(HEADING_SIZE * scale)
            .strong(),
    );
    column.add_space(GAP * 0.75);
    column.label(egui::RichText::new(DESCRIPTION).size(BODY_SIZE * scale));
    column.add_space(GAP);
    if column
        .button(egui::RichText::new("Why Larrygram?").size(BODY_SIZE * scale))
        .clicked()
    {
        open_about = true;
        close_about = false;
    }

    if open_about {
        let mut window_open = true;
        let mut about_error = None;
        let screen = ui.ctx().content_rect();
        egui::Window::new(ABOUT_TITLE)
            .open(&mut window_open)
            .resizable(false)
            .collapsible(false)
            .id(egui::Id::unique(crate::window_size::ABOUT_WINDOW).with("wizard"))
            .fixed_size(crate::window_size::ABOUT.size(screen.size()))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ui.ctx(), |ui| match about_window(ui) {
                Ok(about::Outcome::Open) => {}
                Ok(about::Outcome::Close) => close_about = true,
                Ok(about::Outcome::WatchVideo) => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(about::VIDEO_URL));
                }
                Err(error) => about_error = Some(error),
            });
        open_about = window_open && !close_about;
        state.about_open = open_about;
        if let Some(error) = about_error {
            return Err(error);
        }
    } else {
        state.about_open = false;
    }

    let mut buttons = region(ui, footer, egui::Layout::right_to_left(egui::Align::Center));
    if footer_button(&mut buttons, "Next", scale).clicked() {
        state.page = Page::ChooseLayout;
        state.about_open = false;
    }
    if footer_button(&mut buttons, "Cancel", scale).clicked() {
        state.about_open = false;
        outcome = Some(Outcome::Cancel);
    }

    Ok(outcome)
}

/// Draws page 2 (the two layout choices) and returns its outcome.
///
/// The heading runs across the top; the two pictures share the body
/// in two equal columns at the art's aspect ratio, each with its name
/// button under it; Back and Start sit in the footer.
fn choose_layout_page(ui: &mut Ui, state: &mut State) -> Option<Outcome> {
    use crate::window_size::{GAP, split_footer, text_scale};

    let mut outcome = None;
    let mut layout = state.default_layout;
    let rect = frame_rect(ui);
    let (body, footer) = split_footer(rect);
    let scale = text_scale(rect.width());

    let mut heading = region(ui, body, egui::Layout::top_down(egui::Align::Min));
    heading.add_space(GAP * 0.5);
    heading.label(
        egui::RichText::new(LAYOUT_HEADING)
            .size(HEADING_SIZE * scale)
            .strong(),
    );
    let top = heading.min_rect().max.y + GAP;
    let choices = egui::Rect::from_min_max(egui::pos2(body.min.x, top), body.max);
    let column_width = ((choices.width() - GAP) * 0.5).max(0.0);
    // The two choices sit in the middle of the space under the heading.
    let block =
        (column_width / ART_ASPECT + CHOICE_GAP + CHOICE_BUTTON_HEIGHT).min(choices.height());
    let choices = egui::Rect::from_min_size(
        egui::pos2(
            choices.min.x,
            choices.min.y + ((choices.height() - block) * 0.5).max(0.0),
        ),
        egui::vec2(choices.width(), block),
    );
    let columns = [
        egui::Rect::from_min_size(choices.min, egui::vec2(column_width, choices.height())),
        egui::Rect::from_min_size(
            egui::pos2(choices.min.x + column_width + GAP, choices.min.y),
            egui::vec2(column_width, choices.height()),
        ),
    ];
    let [modern_rect, classic_rect] = columns;
    let modern = layout_choice(
        ui,
        modern_rect,
        "Panel Modern",
        egui::include_image!("../assets/brand/panel-layout.svg"),
        "Panel Modern layout picture",
        layout == LayoutKind::PanelModern,
    );
    let classic = layout_choice(
        ui,
        classic_rect,
        "Panel Classic",
        egui::include_image!("../assets/brand/classic-layout.svg"),
        "Panel Classic layout picture",
        layout == LayoutKind::PanelClassic,
    );
    if modern {
        layout = LayoutKind::PanelModern;
    }
    if classic {
        layout = LayoutKind::PanelClassic;
    }
    state.default_layout = layout;

    let mut buttons = region(ui, footer, egui::Layout::right_to_left(egui::Align::Center));
    if footer_button(&mut buttons, "Start", scale).clicked() {
        outcome = Some(Outcome::Start {
            default_layout: state.default_layout,
        });
    }
    if footer_button(&mut buttons, "Back", scale).clicked() {
        state.page = Page::Welcome;
    }

    outcome
}

/// Draws one large layout button in `column`: its picture at the art's
/// aspect ratio, with the name under it at the picture's width.
///
/// Returns `true` when the picture or the name was clicked. The name is
/// a selectable button, so it carries the accessible name and the
/// selected state; the picture is clickable too.
fn layout_choice(
    ui: &mut Ui,
    column: egui::Rect,
    name: &str,
    image: egui::ImageSource<'static>,
    alt_text: &str,
    selected: bool,
) -> bool {
    use crate::window_size::fit_aspect;

    let picture_box = egui::Rect::from_min_max(
        column.min,
        egui::pos2(
            column.max.x,
            (column.max.y - CHOICE_BUTTON_HEIGHT - CHOICE_GAP).max(column.min.y),
        ),
    );
    let mut picture_rect = fit_aspect(picture_box, ART_ASPECT);
    // The picture sits at the top of its box, the button right under it.
    picture_rect = picture_rect.translate(egui::vec2(0.0, picture_box.min.y - picture_rect.min.y));
    let picture = ui.put(
        picture_rect,
        egui::Image::new(image)
            .fit_to_exact_size(picture_rect.size())
            .sense(egui::Sense::click())
            .alt_text(alt_text),
    );
    let button_rect = egui::Rect::from_min_size(
        egui::pos2(picture_rect.min.x, picture_rect.max.y + CHOICE_GAP),
        egui::vec2(picture_rect.width(), CHOICE_BUTTON_HEIGHT),
    );
    let button = ui.put(button_rect, egui::Button::selectable(selected, name));
    picture.clicked() || button.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;

    #[test]
    fn state_starts_on_the_welcome_page_with_panel_modern() {
        let state = State::new();
        assert_eq!(state.page(), Page::Welcome);
        assert_eq!(state.default_layout(), LayoutKind::PanelModern);
        assert_eq!(State::default(), state);
    }

    #[test]
    fn the_wizard_draws_without_not_implemented() {
        let mut state = State::new();
        let outcome = with_ui(|ui| show(ui, &mut state));
        assert!(matches!(outcome, Ok(None)));
    }
}
