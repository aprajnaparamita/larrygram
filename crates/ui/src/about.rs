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
/// Left: `larry.png`, square, as tall as the body and never wider
/// than half of it. Right, with padding: the dedication text and,
/// centred, the video button. Bottom: Close, in the footer row. The
/// layout fills the ui it is given (the shell sizes the window with
/// [`window_size::ABOUT`](crate::window_size::ABOUT)), and the text
/// grows with it (task P7-17). Close wins when both buttons report a
/// click in the same frame.
pub fn show(ui: &mut Ui) -> Result<Outcome, UiError> {
    use crate::window_size::{GAP, fit_aspect, split_footer, text_scale};

    /// The body text size at a text scale of 1, in points.
    const BODY_SIZE: f32 = 14.0;

    let rect = ui.available_rect_before_wrap();
    ui.allocate_rect(rect, egui::Sense::hover());
    let (body, footer) = split_footer(rect);
    let scale = text_scale(rect.width());
    let photo_box = egui::Rect::from_min_size(
        body.min,
        egui::vec2((body.width() * 0.5).min(body.height()), body.height()),
    );
    let mut photo = fit_aspect(photo_box, 1.0);
    // Top-aligned, so the text and the photo start on one line.
    photo = photo.translate(egui::vec2(0.0, photo_box.min.y - photo.min.y));
    ui.put(
        photo,
        egui::Image::new(egui::include_image!("../assets/brand/larry.png"))
            .fit_to_exact_size(photo.size())
            .alt_text(PICTURE_ALT),
    );

    let mut watch_video = false;
    let text = egui::Rect::from_min_max(
        egui::pos2(photo_box.max.x + GAP, body.min.y),
        egui::pos2(body.max.x - GAP * 0.5, body.max.y),
    );
    let mut column = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let body_text = |text: &str| egui::RichText::new(text).size(BODY_SIZE * scale);
    column.label(body_text(DEDICATION).strong());
    column.add_space(GAP * 0.75);
    column.label(body_text(STORY));
    column.add_space(GAP);
    column.vertical_centered(|ui| {
        if ui.button(body_text(VIDEO_BUTTON)).clicked() {
            watch_video = true;
        }
    });
    column.add_space(GAP);
    column.label(body_text(CLOSING));

    let mut close = false;
    let mut buttons = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(footer)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if buttons
        .add(egui::Button::new(body_text("Close")).min_size(egui::vec2(80.0 * scale, 0.0)))
        .clicked()
    {
        close = true;
    }

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
