//! The About window (ADR 0066, `docs/phase-7/README.md`).
//!
//! It opens from **Why Larrygram?** in the welcome wizard and from
//! **Help > About Larrygram** in every layout. [`show`] draws one
//! frame: `larry.png` on the left, the dedication and the video
//! button on the right, and **Close** at the bottom. The caller opens
//! [`VIDEO_URL`] through egui's open-URL output when the frame
//! returns [`Outcome::WatchVideo`].

use egui::Ui;

use crate::UiError;

/// The video link of the About window.
///
/// The `?si=...` share-tracking id is deliberately absent. The
/// button opens this URL through egui's open-URL output, which works
/// on desktop and on web.
pub const VIDEO_URL: &str = "https://youtu.be/wpgiRvcGY_U";

/// The dedication line (ADR 0066, verbatim).
const DEDICATION: &str = "This app is dedicated to Larry Jeffus.";

/// Larry's story (ADR 0066, verbatim).
const STORY: &str = "Larry is an engineer with a deeply inspiring story. He began life with severe dyslexia, and although he could not read, he went on to write more than 40 books and to teach hundreds of thousands of students around the world.";

/// The video button label (ADR 0066, verbatim).
const VIDEO_BUTTON: &str = "[ Watch Larry's story ]";

/// The closing paragraph (ADR 0066, verbatim).
const CLOSING: &str = "He loves to make all his engineering designs with diagramming applications. That is why his niece, Janet Jeffus, decided to make this app and dedicate it to his inspiring story.";

/// The picture on the left (ADR 0067), with its accessible name.
const PICTURE_ALT: &str = "Larry Jeffus";

/// What the caller must do after a [`show`] frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The window is open and nothing was chosen this frame.
    Open,
    /// Close was pressed: hide the window.
    Close,
    /// The video button was pressed: open [`VIDEO_URL`] through egui's
    /// open-URL output.
    WatchVideo,
}

/// Draws one frame of the About window (ADR 0066).
///
/// Left half: `larry.png` scaled to fit and keeping its aspect
/// ratio. Right half, with generous padding: the
/// dedication text and, centred, the video button. Bottom: Close.
/// Close wins when both buttons report a click in the same frame.
pub fn show(ui: &mut Ui) -> Result<Outcome, UiError> {
    /// The footer height in points, left free under the two columns.
    const FOOTER_HEIGHT: f32 = 40.0;
    /// The padding inside the right column, in points.
    const PADDING: f32 = 16.0;

    let mut watch_video = false;
    let height = (ui.available_height() - FOOTER_HEIGHT).max(200.0);
    let content_size = egui::vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(
        content_size,
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.columns(2, |columns| {
                columns[0].vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/brand/larry.png"))
                            .fit_to_fraction(egui::vec2(1.0, 1.0))
                            .maintain_aspect_ratio(true)
                            .alt_text(PICTURE_ALT),
                    );
                });

                columns[1].vertical(|ui| {
                    ui.add_space(PADDING);
                    ui.label(DEDICATION);
                    ui.add_space(12.0);
                    ui.label(STORY);
                    ui.add_space(PADDING);
                    ui.vertical_centered(|ui| {
                        if ui.button(VIDEO_BUTTON).clicked() {
                            watch_video = true;
                        }
                    });
                    ui.add_space(PADDING);
                    ui.label(CLOSING);
                    ui.add_space(PADDING);
                });
            });
        },
    );

    let mut close = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button("Close").clicked() {
            close = true;
        }
    });

    if close {
        Ok(Outcome::Close)
    } else if watch_video {
        Ok(Outcome::WatchVideo)
    } else {
        Ok(Outcome::Open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::with_ui;

    #[test]
    fn video_url_has_no_tracking_id() {
        assert_eq!(VIDEO_URL, "https://youtu.be/wpgiRvcGY_U");
        assert!(!VIDEO_URL.contains("si="));
    }

    #[test]
    fn show_draws_and_stays_open() {
        let outcome = with_ui(show).expect("show draws");
        assert_eq!(outcome, Outcome::Open);
    }
}
