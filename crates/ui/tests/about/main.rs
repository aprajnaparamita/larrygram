//! The About window (task P7-16, ADR 0066).
//!
//! The tests drive the real widget through `egui_kittest`, with the
//! ADR 0067 image loaders installed on the harness context.

// The never-crash lints apply to non-test code only (AGENTS.md section 6).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui_kittest::{Harness, kittest::Queryable};
use larrygram_ui::about::{self, Outcome};

/// The dedication text of the About window (ADR 0066, verbatim).
const DEDICATION: &str = "This app is dedicated to Larry Jeffus.";
const STORY: &str = "Larry is an engineer with a deeply inspiring story. He began life with severe dyslexia, and although he could not read, he went on to write more than 40 books and to teach hundreds of thousands of students around the world.";
const CLOSING: &str = "He loves to make all his engineering designs with diagramming applications. That is why his niece, Janet Jeffus, decided to make this app and dedicate it to his inspiring story.";
/// The video button label (ADR 0066, verbatim).
const VIDEO_BUTTON: &str = "[ Watch Larry's story ]";

/// An About window driven headlessly.
struct Rig {
    outcome: Rc<RefCell<Option<Outcome>>>,
    harness: Harness<'static>,
}

impl Rig {
    /// Builds a rig and draws the first frame.
    fn new() -> Self {
        let outcome = Rc::new(RefCell::new(Some(Outcome::Open)));
        let harness = Harness::builder()
            .with_size(egui::vec2(900.0, 600.0))
            .with_context_setup(larrygram_ui::install_image_loaders)
            .build_ui({
                let outcome = Rc::clone(&outcome);
                move |ui| {
                    if let Ok(current) = about::show(ui) {
                        // Latch: a click frame is followed by idle
                        // frames that answer Open again.
                        if current != Outcome::Open {
                            *outcome.borrow_mut() = Some(current);
                        }
                    }
                }
            });
        let mut rig = Self { outcome, harness };
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

    /// The outcome of the last frame, if any.
    fn outcome(&self) -> Option<Outcome> {
        *self.outcome.borrow()
    }
}

#[test]
fn the_dedication_text_renders_verbatim() {
    let rig = Rig::new();
    let root = rig.harness.root();
    assert!(
        root.query_all_by_label(DEDICATION).next().is_some(),
        "the dedication line renders"
    );
    assert!(
        root.query_all_by_label(STORY).next().is_some(),
        "the ADR 0066 story paragraph renders verbatim"
    );
    assert!(
        root.query_all_by_label(CLOSING).next().is_some(),
        "the ADR 0066 closing paragraph renders verbatim"
    );
    assert!(
        root.query_all_by_role_and_label(Role::Button, VIDEO_BUTTON)
            .next()
            .is_some(),
        "the video button renders"
    );
    assert!(
        root.query_all_by_role_and_label(Role::Button, "Close")
            .next()
            .is_some(),
        "Close renders"
    );
}

#[test]
fn the_video_button_returns_watch_video_without_a_tracking_id() {
    let mut rig = Rig::new();
    assert_eq!(rig.outcome(), Some(Outcome::Open));
    rig.click(VIDEO_BUTTON);
    assert_eq!(rig.outcome(), Some(Outcome::WatchVideo));
    assert_eq!(about::VIDEO_URL, "https://youtu.be/wpgiRvcGY_U");
    assert!(!about::VIDEO_URL.contains("si="), "no share-tracking id");
}

#[test]
fn close_returns_close() {
    let mut rig = Rig::new();
    rig.click("Close");
    assert_eq!(rig.outcome(), Some(Outcome::Close));
}
