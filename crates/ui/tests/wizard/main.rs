//! The welcome wizard (task P7-15, ADR 0066).
//!
//! The tests drive the real widget through `egui_kittest`, with the
//! ADR 0067 image loaders installed on the harness context. The About
//! window is faked through the public
//! [`show_with_about`](larrygram_ui::wizard::show_with_about) seam so
//! the wizard tests do not depend on task P7-16.

// The never-crash lints apply to non-test code only (AGENTS.md section 6).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};
use larrygram_ui::about;
use larrygram_ui::wizard::{self, Outcome, Page, State};
use larrygram_ui_spec::v2::LayoutKind;

/// The title and description of page 1 (ADR 0066, verbatim).
const TITLE: &str = "Larrygram v1.0 alpha";
const DESCRIPTION: &str = "Larrygram is a free, open-source diagramming app. Draw flowcharts and diagrams with shapes and connectors, and open, edit and save .vsdx drawings, on the desktop or in your browser.";

/// A wizard driven headlessly, with a fake About window.
struct Rig {
    state: Rc<RefCell<State>>,
    outcome: Rc<RefCell<Option<Outcome>>>,
    about_action: Rc<Cell<about::Outcome>>,
    about_calls: Rc<Cell<usize>>,
    harness: Harness<'static>,
}

impl Rig {
    /// Builds a rig; the fake About window returns `about_action`.
    fn new(about_action: about::Outcome) -> Self {
        let state = Rc::new(RefCell::new(State::new()));
        let outcome = Rc::new(RefCell::new(None));
        let about_action = Rc::new(Cell::new(about_action));
        let about_calls = Rc::new(Cell::new(0));

        let harness = Harness::builder()
            .with_size(egui::vec2(1000.0, 700.0))
            .with_context_setup(larrygram_ui::install_image_loaders)
            .build_ui({
                let state = Rc::clone(&state);
                let outcome = Rc::clone(&outcome);
                let about_action = Rc::clone(&about_action);
                let about_calls = Rc::clone(&about_calls);
                move |ui| {
                    let result = {
                        let mut state = state.borrow_mut();
                        wizard::show_with_about(ui, &mut state, |ui| {
                            about_calls.set(about_calls.get() + 1);
                            ui.label("Fake About window");
                            Ok(about_action.get())
                        })
                    };
                    if let Ok(Some(outcome_now)) = result {
                        *outcome.borrow_mut() = Some(outcome_now);
                    }
                }
            });

        let mut rig = Self {
            state,
            outcome,
            about_action,
            about_calls,
            harness,
        };
        rig.run();
        rig
    }

    /// One harness frame series.
    fn run(&mut self) {
        self.harness.run();
    }

    /// Clicks a button by its accessible name.
    fn click(&mut self, label: &str) {
        {
            let root = self.harness.root();
            let node = root
                .query_all_by_role_and_label(Role::Button, label)
                .next()
                .unwrap_or_else(|| panic!("the {label:?} button renders"));
            node.click();
        }
        self.run();
    }

    /// The current wizard page.
    fn page(&self) -> Page {
        self.state.borrow().page()
    }

    /// The current layout selection.
    fn layout(&self) -> LayoutKind {
        self.state.borrow().default_layout()
    }

    /// The outcome of the last frame, if any.
    fn outcome(&self) -> Option<Outcome> {
        *self.outcome.borrow()
    }
}

#[test]
fn the_welcome_page_shows_its_copy_and_the_layout_page_flows() {
    let mut rig = Rig::new(about::Outcome::Open);
    assert_eq!(rig.page(), Page::Welcome);

    {
        let root = rig.harness.root();
        assert!(
            root.query_all_by_label(TITLE).next().is_some(),
            "the title renders"
        );
        assert!(
            root.query_all_by_label(DESCRIPTION).next().is_some(),
            "the ADR 0066 description renders verbatim"
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Why Larrygram?")
                .next()
                .is_some()
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Next")
                .next()
                .is_some()
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Cancel")
                .next()
                .is_some()
        );
    }

    rig.click("Next");
    assert_eq!(rig.page(), Page::ChooseLayout);
    assert_eq!(rig.layout(), LayoutKind::PanelModern);
    {
        let root = rig.harness.root();
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Panel Modern")
                .next()
                .is_some(),
            "the Modern choice renders"
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Panel Classic")
                .next()
                .is_some(),
            "the Classic choice renders"
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Back")
                .next()
                .is_some()
        );
        assert!(
            root.query_all_by_role_and_label(Role::Button, "Start")
                .next()
                .is_some()
        );
    }

    rig.click("Panel Classic");
    assert_eq!(rig.layout(), LayoutKind::PanelClassic);

    rig.click("Back");
    assert_eq!(rig.page(), Page::Welcome);

    rig.click("Next");
    rig.click("Start");
    assert_eq!(
        rig.outcome(),
        Some(Outcome::Start {
            default_layout: LayoutKind::PanelClassic
        })
    );
}

#[test]
fn cancel_on_the_welcome_page_returns_cancel() {
    let mut rig = Rig::new(about::Outcome::Open);
    rig.click("Cancel");
    assert_eq!(rig.outcome(), Some(Outcome::Cancel));
}

#[test]
fn start_with_the_default_selection_returns_panel_modern() {
    let mut rig = Rig::new(about::Outcome::Open);
    rig.click("Next");
    rig.click("Start");
    assert_eq!(
        rig.outcome(),
        Some(Outcome::Start {
            default_layout: LayoutKind::PanelModern
        })
    );
}

#[test]
fn why_larrygram_opens_the_about_window() {
    let mut rig = Rig::new(about::Outcome::Open);
    assert_eq!(rig.about_calls.get(), 0);

    rig.click("Why Larrygram?");
    assert!(
        rig.about_calls.get() > 0,
        "the fake About window was called"
    );
    {
        let root = rig.harness.root();
        assert!(
            root.query_all_by_label("Fake About window")
                .next()
                .is_some(),
            "the About window content renders"
        );
    }
    assert_eq!(rig.page(), Page::Welcome, "the wizard stays on page 1");
}

#[test]
fn the_about_window_closes_without_closing_the_wizard() {
    let mut rig = Rig::new(about::Outcome::Open);
    rig.click("Why Larrygram?");
    assert!(rig.about_calls.get() > 0);

    rig.about_action.set(about::Outcome::Close);
    rig.run();
    rig.run();
    {
        let root = rig.harness.root();
        assert!(
            root.query_all_by_label("Fake About window")
                .next()
                .is_none(),
            "the About window is gone after Outcome::Close"
        );
    }
    assert_eq!(rig.page(), Page::Welcome);
}

#[test]
fn watch_video_is_forwarded_to_the_platform_output() {
    let mut rig = Rig::new(about::Outcome::Open);
    rig.click("Why Larrygram?");
    rig.about_action.set(about::Outcome::WatchVideo);
    rig.run();

    let commands = &rig.harness.output().platform_output.commands;
    assert!(
        commands.iter().any(|command| matches!(
            command,
            egui::OutputCommand::OpenUrl(url) if url.url == about::VIDEO_URL
        )),
        "the video URL is opened through egui's open-URL output: {commands:?}"
    );
}

#[test]
fn an_about_error_is_propagated() {
    let state = Rc::new(RefCell::new(State::new()));
    let error = Rc::new(RefCell::new(None));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_context_setup(larrygram_ui::install_image_loaders)
        .build_ui({
            let state = Rc::clone(&state);
            let error = Rc::clone(&error);
            move |ui| {
                let mut state = state.borrow_mut();
                if let Err(error_now) = wizard::show_with_about(ui, &mut state, |_ui| {
                    Err(larrygram_ui::UiError::Io {
                        message: "about failed".to_owned(),
                    })
                }) {
                    *error.borrow_mut() = Some(error_now.to_string());
                }
            }
        });
    harness.run();
    {
        let root = harness.root();
        let node = root
            .query_all_by_role_and_label(Role::Button, "Why Larrygram?")
            .next()
            .expect("the Why button renders");
        node.click();
    }
    harness.run();
    assert_eq!(
        error.borrow().as_deref(),
        Some("io error: about failed"),
        "the About error reaches the caller"
    );
}
