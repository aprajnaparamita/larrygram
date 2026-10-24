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

/// Draws page 1 and returns its outcome.
fn welcome_page<F>(
    ui: &mut Ui,
    state: &mut State,
    mut about_window: F,
) -> Result<Option<Outcome>, UiError>
where
    F: FnMut(&mut Ui) -> Result<about::Outcome, UiError>,
{
    let mut outcome = None;
    let mut open_about = state.about_open;
    let mut close_about = false;

    let content_size = content_size(ui);
    ui.allocate_ui_with_layout(
        content_size,
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.columns(2, |columns| {
                columns[0].vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/brand/about_screen.svg"))
                            .fit_to_fraction(egui::vec2(1.0, 1.0))
                            .maintain_aspect_ratio(true)
                            .alt_text("Larrygram welcome picture"),
                    );
                });

                columns[1].vertical(|ui| {
                    ui.add_space(16.0);
                    ui.heading(TITLE);
                    ui.add_space(12.0);
                    ui.label(DESCRIPTION);
                    ui.add_space(18.0);
                    if ui.button("Why Larrygram?").clicked() {
                        open_about = true;
                        close_about = false;
                    }
                });
            });
        },
    );

    if open_about {
        let mut window_open = true;
        let mut about_error = None;
        egui::Window::new(ABOUT_TITLE)
            .open(&mut window_open)
            .resizable(false)
            .collapsible(false)
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

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Next").clicked() {
            state.page = Page::ChooseLayout;
            state.about_open = false;
        }
        if ui.button("Cancel").clicked() {
            state.about_open = false;
            outcome = Some(Outcome::Cancel);
        }
    });

    Ok(outcome)
}

/// Draws page 2 (the two layout choices) and returns its outcome.
fn choose_layout_page(ui: &mut Ui, state: &mut State) -> Option<Outcome> {
    let mut outcome = None;
    let mut layout = state.default_layout;
    let content_size = content_size(ui);
    let (modern, classic) = ui
        .allocate_ui_with_layout(
            content_size,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.columns_const::<2, _>(|columns| {
                    let modern = layout_choice(
                        &mut columns[0],
                        "Panel Modern",
                        egui::include_image!("../assets/brand/panel-layout.svg"),
                        "Panel Modern layout picture",
                        layout == LayoutKind::PanelModern,
                    );
                    let classic = layout_choice(
                        &mut columns[1],
                        "Panel Classic",
                        egui::include_image!("../assets/brand/classic-layout.svg"),
                        "Panel Classic layout picture",
                        layout == LayoutKind::PanelClassic,
                    );
                    (modern, classic)
                })
            },
        )
        .inner;
    if modern {
        layout = LayoutKind::PanelModern;
    }
    if classic {
        layout = LayoutKind::PanelClassic;
    }
    state.default_layout = layout;

    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Start").clicked() {
            outcome = Some(Outcome::Start {
                default_layout: state.default_layout,
            });
        }
        if ui.button("Back").clicked() {
            state.page = Page::Welcome;
        }
    });

    outcome
}

/// The content area above the bottom row, leaving room for the
/// footer buttons inside small windows too.
fn content_size(ui: &Ui) -> egui::Vec2 {
    const FOOTER_HEIGHT: f32 = 40.0;
    let height = (ui.available_height() - FOOTER_HEIGHT).max(160.0);
    egui::vec2(ui.available_width(), height)
}

/// Draws one large layout button: its picture with the name under it.
///
/// Returns `true` when the picture or the name was clicked. The name is
/// a selectable button, so it carries the accessible name and the
/// selected state; the picture is clickable too.
fn layout_choice(
    ui: &mut Ui,
    name: &str,
    image: egui::ImageSource<'static>,
    alt_text: &str,
    selected: bool,
) -> bool {
    let mut clicked = false;
    ui.vertical(|ui| {
        let picture = ui.add(
            egui::Image::new(image)
                .max_height(240.0)
                .fit_to_fraction(egui::vec2(1.0, 1.0))
                .maintain_aspect_ratio(true)
                .sense(egui::Sense::click())
                .alt_text(alt_text),
        );
        clicked |= picture.clicked();
        let button_width = ui.available_width().min(320.0);
        clicked |= ui
            .add_sized(
                [button_width, 32.0],
                egui::Button::selectable(selected, name),
            )
            .clicked();
    });
    clicked
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
