//! The Page Setup dialog (task P7-21): page size, orientation and
//! measurement units over P7-20's `tools.options.page.setup` command.
//!
//! The dialog answers [`UiRequest::ShowPageSetup`]
//! (larrygram_commands): with no arguments the command requests it,
//! and the dialog re-dispatches the same command with `width` and
//! `height` (inches), `orientation` (`portrait` / `landscape`) and
//! `units` (a [`DisplayUnits`] name). The dialog itself changes
//! nothing: one command writes the page (ADR 0028).
//!
//! The style is the P7-18 dialog rule: the bordered check boxes and
//! radio buttons of [`theme::raise_controls`], the accent default
//! button of [`theme::accent_controls`], 16 px margins and the
//! `title` / `ui` roles.

use egui::{Align, Layout, RichText, Ui};
use larrygram_commands::app::{AppCtx, DisplayUnits};
use larrygram_commands::{ArgValue, Args};

use crate::theme;

/// Which side of the page is longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Tall.
    Portrait,
    /// Wide.
    Landscape,
}

impl Orientation {
    /// The command's `orientation` argument spelling.
    pub fn value(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }
}

/// The dialog state: the fields as the user edits them.
#[derive(Debug, Clone, PartialEq)]
pub struct State {
    /// The page width, in inches, as typed.
    pub width: String,
    /// The page height, in inches, as typed.
    pub height: String,
    /// The orientation radios.
    pub orientation: Orientation,
    /// The measurement units row (a [`DisplayUnits`] name).
    pub units: &'static str,
    /// The message of the last rejected edit ("width must be a
    /// positive number"), cleared by the next accepted one.
    pub error: Option<String>,
}

impl State {
    /// The dialog over the active page's size (inches), falling back to
    /// the default Letter size when no document is open.
    pub fn from_app(app: &AppCtx) -> Self {
        let (width, height) = app
            .active_document()
            .and_then(|session| session.active_page)
            .and_then(|page| {
                let doc = &app.active_document()?.document;
                let sheet = &doc.pages.get(page.index as usize)?.sheet;
                let number = |name: &str| -> Option<f64> {
                    sheet
                        .cells
                        .iter()
                        .find(|cell| cell.name.as_str() == name)
                        .and_then(|cell| match cell.value {
                            Some(larrygram_model::CellValue::Number(value)) => Some(value),
                            _ => None,
                        })
                };
                Some((number("PageWidth")?, number("PageHeight")?))
            })
            .filter(|(width, height)| *width > 0.0 && *height > 0.0)
            .unwrap_or((8.5, 11.0));
        let orientation = if width > height {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        };
        Self {
            width: trim_number(width),
            height: trim_number(height),
            orientation,
            units: "Inches",
            error: None,
        }
    }

    /// The accepted arguments of the current fields, or the reason a
    /// field did not parse.
    pub fn args(&self) -> Result<Args, String> {
        let width = parse_size("width", &self.width)?;
        let height = parse_size("height", &self.height)?;
        Ok(Args::new()
            .with("width", ArgValue::Number(width))
            .with("height", ArgValue::Number(height))
            .with(
                "orientation",
                ArgValue::Text(self.orientation.value().to_owned()),
            )
            .with("units", ArgValue::Text(self.units.to_owned())))
    }
}

/// One size field as a positive number of inches.
fn parse_size(name: &str, text: &str) -> Result<f64, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("{name} must be a positive number"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{name} must be a positive number"));
    }
    Ok(value)
}

/// The shortest spelling of a size ("8.5", "11").
fn trim_number(value: f64) -> String {
    format!("{value}")
}

/// What the dialog did this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Nothing yet (or a field that did not parse).
    None,
    /// The user pressed OK: here are the command arguments.
    Apply(Args),
    /// The user cancelled (or closed the window).
    Cancel,
}

/// Draws the dialog body. The host owns the [`egui::Window`] and its
/// close button; this draws the sections and the buttons.
pub fn show(ui: &mut Ui, state: &mut State) -> Outcome {
    // The bordered check boxes and radio buttons of the design pass
    // (P7-18's `raise_controls`).
    theme::raise_controls(ui);
    let mut outcome = Outcome::None;

    ui.label(RichText::new("Page size").strong());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Width:");
        ui.add(egui::TextEdit::singleline(&mut state.width).desired_width(64.0))
            .accessible_name("Width");
        ui.label("in");
    });
    ui.horizontal(|ui| {
        ui.label("Height:");
        ui.add(egui::TextEdit::singleline(&mut state.height).desired_width(64.0))
            .accessible_name("Height");
        ui.label("in");
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Orientation").strong());
    ui.horizontal(|ui| {
        ui.radio_value(&mut state.orientation, Orientation::Portrait, "Portrait");
        ui.radio_value(&mut state.orientation, Orientation::Landscape, "Landscape");
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Measurement units").strong());
    for name in DisplayUnits::NAMES {
        ui.radio_value(&mut state.units, name, name);
    }

    ui.add_space(12.0);
    if let Some(message) = &state.error {
        ui.colored_label(theme::color_at(ui.ctx(), "error"), message.as_str());
    }
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            // The default button of a dialog is the accent one
            // (P7-18's `accent_controls`).
            theme::accent_controls(ui);
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
            if ui.button("OK").clicked() {
                match state.args() {
                    Ok(args) => outcome = Outcome::Apply(args),
                    Err(message) => state.error = Some(message),
                }
            }
        });
    });
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};
    use larrygram_commands::app::Settings;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn the_state_reads_the_active_page_size() {
        let app = AppCtx::new(Settings::default());
        let state = State::from_app(&app);
        assert_eq!(state.width, "8.5");
        assert_eq!(state.height, "11");
        assert_eq!(state.orientation, Orientation::Portrait);
        assert_eq!(state.units, "Inches");
    }

    #[test]
    fn the_args_carry_the_command_contract() {
        let state = State {
            width: "11".to_owned(),
            height: "8.5".to_owned(),
            orientation: Orientation::Landscape,
            units: "Millimeters",
            error: None,
        };
        let args = state.args().expect("the fields parse");
        assert_eq!(args.number("width"), Some(11.0));
        assert_eq!(args.number("height"), Some(8.5));
        assert_eq!(args.text("orientation"), Some("landscape"));
        assert_eq!(args.text("units"), Some("Millimeters"));
    }

    #[test]
    fn a_bad_size_is_an_error_not_a_crash() {
        let state = State {
            width: "wide".to_owned(),
            height: "-2".to_owned(),
            orientation: Orientation::Portrait,
            units: "Inches",
            error: None,
        };
        let error = state.args().expect_err("the fields do not parse");
        assert!(error.contains("width"), "{error}");
    }

    /// One dialog drawn over a shared state and outcome.
    fn harness_of(state: &Rc<RefCell<State>>, outcome: &Rc<RefCell<Outcome>>) -> Harness<'static> {
        let state = Rc::clone(state);
        let outcome = Rc::clone(outcome);
        Harness::builder()
            .with_size(egui::vec2(320.0, 480.0))
            .build_ui(move |ui| {
                let mut state = state.borrow_mut();
                // Keep the last non-idle outcome: the closure re-runs
                // every frame and an idle frame would clear it.
                let produced = show(ui, &mut state);
                if produced != Outcome::None {
                    *outcome.borrow_mut() = produced;
                }
            })
    }

    /// Clicks the button `label` in one dialog.
    fn click_button(harness: &mut Harness<'static>, label: &str) {
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(egui::accesskit::Role::Button, label)
                .next()
                .unwrap_or_else(|| panic!("the {label} button draws"));
            node.click();
        }
        harness.run();
    }

    #[test]
    fn ok_applies_and_cancel_cancels() {
        let state = Rc::new(RefCell::new(State::from_app(&AppCtx::new(
            Settings::default(),
        ))));
        let outcome = Rc::new(RefCell::new(Outcome::None));
        let mut harness = harness_of(&state, &outcome);
        harness.run();
        click_button(&mut harness, "OK");
        assert!(
            matches!(*outcome.borrow(), Outcome::Apply(_)),
            "OK applies (got {:?})",
            *outcome.borrow()
        );

        let state = Rc::new(RefCell::new(State::from_app(&AppCtx::new(
            Settings::default(),
        ))));
        let outcome = Rc::new(RefCell::new(Outcome::None));
        let mut harness = harness_of(&state, &outcome);
        harness.run();
        click_button(&mut harness, "Cancel");
        assert_eq!(*outcome.borrow(), Outcome::Cancel);
    }

    #[test]
    fn the_orientation_radios_work_by_mouse() {
        let state = Rc::new(RefCell::new(State::from_app(&AppCtx::new(
            Settings::default(),
        ))));
        let outcome = Rc::new(RefCell::new(Outcome::None));
        let mut harness = harness_of(&state, &outcome);
        harness.run();
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(egui::accesskit::Role::RadioButton, "Landscape")
                .next()
                .expect("the Landscape radio draws");
            node.click();
        }
        harness.run();
        assert_eq!(state.borrow().orientation, Orientation::Landscape);
    }

    #[test]
    fn the_units_rows_work_by_mouse() {
        let state = Rc::new(RefCell::new(State::from_app(&AppCtx::new(
            Settings::default(),
        ))));
        let outcome = Rc::new(RefCell::new(Outcome::None));
        let mut harness = harness_of(&state, &outcome);
        harness.run();
        {
            let root = harness.root();
            let node = root
                .query_all_by_role_and_label(egui::accesskit::Role::RadioButton, "Meters")
                .next()
                .expect("the Meters radio draws");
            node.click();
        }
        harness.run();
        assert_eq!(state.borrow().units, "Meters");
    }
}
