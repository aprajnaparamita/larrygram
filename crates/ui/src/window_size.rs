//! Where the modal windows sit and how large they draw (task P7-17):
//! the welcome wizard, the About window and the Options dialog.
//!
//! A window's size comes from the app window's content rect in
//! points, so it is the same at display scales 1 and 2 and follows the
//! window when it is resized. Each window has a [`SizeRule`]: a share
//! of the app window's width, a fixed aspect ratio (the artwork's), and
//! a minimum and a maximum size. The window never grows past the app
//! window less [`SizeRule::margin`] on each side, even when that means
//! going under its minimum.

use egui::{Rect, Vec2};

/// The height of the footer row that holds a window's buttons.
pub const FOOTER_HEIGHT: f32 = 40.0;

/// The gap between two columns of a window, and between the content
/// and the footer.
pub const GAP: f32 = 16.0;

/// The size rule of one modal window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeRule {
    /// Width over height of the window's content.
    pub aspect: f32,
    /// The share of the app window's width the window takes.
    pub fraction: f32,
    /// The smallest content size, when the app window has room for it.
    pub min: Vec2,
    /// The largest content size.
    pub max: Vec2,
    /// The space kept free between the window and each edge of the
    /// app window (room for the window frame and title bar too).
    pub margin: f32,
}

/// The welcome wizard: 16:10, the aspect of the brand art.
pub const WIZARD: SizeRule = SizeRule {
    aspect: 1.6,
    fraction: 0.7,
    min: Vec2::new(560.0, 350.0),
    max: Vec2::new(1040.0, 650.0),
    margin: 32.0,
};

/// The About window: the square photo on the left, the text on the
/// right, at 16:10 overall.
pub const ABOUT: SizeRule = SizeRule {
    aspect: 1.6,
    fraction: 0.6,
    min: Vec2::new(560.0, 350.0),
    max: Vec2::new(880.0, 550.0),
    margin: 48.0,
};

impl SizeRule {
    /// The content size for an app window of `screen` points.
    ///
    /// The width is [`Self::fraction`] of the screen, clamped to
    /// [`Self::min`] and [`Self::max`], and the height follows from
    /// [`Self::aspect`]. When the result does not fit inside the screen
    /// less the margins, it shrinks, keeping the aspect ratio. A
    /// degenerate screen (zero, negative or not finite) gives the
    /// minimum size.
    pub fn size(&self, screen: Vec2) -> Vec2 {
        let aspect = if self.aspect.is_finite() && self.aspect > 0.0 {
            self.aspect
        } else {
            1.0
        };
        if !(screen.x.is_finite() && screen.y.is_finite()) || screen.x <= 0.0 || screen.y <= 0.0 {
            return self.min;
        }
        let room = (screen - Vec2::splat(2.0 * self.margin)).max(Vec2::splat(1.0));
        let mut width = (screen.x * self.fraction).clamp(self.min.x, self.max.x.max(self.min.x));
        let mut height = width / aspect;
        if height > room.y {
            height = room.y;
            width = height * aspect;
        }
        if width > room.x {
            width = room.x;
            height = width / aspect;
        }
        Vec2::new(width.round(), height.round())
    }

    /// The content rect, centred in `screen`.
    pub fn rect(&self, screen: Rect) -> Rect {
        Rect::from_center_size(screen.center(), self.size(screen.size()))
    }
}

/// Splits a window's content rect into the body and the footer row
/// along its bottom edge.
pub fn split_footer(rect: Rect) -> (Rect, Rect) {
    let footer_top = (rect.max.y - FOOTER_HEIGHT).max(rect.min.y);
    let body = Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x, (footer_top - GAP * 0.5).max(rect.min.y)),
    );
    let footer = Rect::from_min_max(egui::pos2(rect.min.x, footer_top), rect.max);
    (body, footer)
}

/// The largest rect of `aspect` (width over height) that fits in
/// `rect`, centred in it. A degenerate aspect gives `rect` itself.
pub fn fit_aspect(rect: Rect, aspect: f32) -> Rect {
    if !(aspect.is_finite() && aspect > 0.0) || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return rect;
    }
    let width = rect.width().min(rect.height() * aspect);
    let size = Vec2::new(width, width / aspect);
    Rect::from_center_size(rect.center(), size)
}

/// The text scale of a window `width` points wide: 1 at 700 pt,
/// between 0.9 and 1.3.
pub fn text_scale(width: f32) -> f32 {
    if width.is_finite() {
        (width / 700.0).clamp(0.9, 1.3)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_footer_sits_along_the_bottom_edge() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(400.0, 300.0));
        let (body, footer) = split_footer(rect);
        assert_eq!(footer.max, rect.max);
        assert_eq!(footer.height(), FOOTER_HEIGHT);
        assert!(body.max.y < footer.min.y);
        assert_eq!(body.min, rect.min);
        // A rect shorter than the footer keeps everything inside it.
        let tiny = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(100.0, 10.0));
        let (body, footer) = split_footer(tiny);
        assert!(tiny.contains_rect(body) && tiny.contains_rect(footer));
    }

    #[test]
    fn fit_aspect_keeps_the_aspect_and_centres() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 100.0));
        let fitted = fit_aspect(rect, 2.0);
        assert_eq!(fitted.size(), Vec2::new(200.0, 100.0));
        assert_eq!(fitted.center(), rect.center());
        let tall = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(100.0, 400.0));
        assert_eq!(fit_aspect(tall, 1.0).size(), Vec2::new(100.0, 100.0));
        assert_eq!(fit_aspect(rect, f32::NAN), rect);
        assert_eq!(fit_aspect(Rect::NOTHING, 1.0), Rect::NOTHING);
    }

    #[test]
    fn the_text_scale_is_bounded() {
        assert_eq!(text_scale(700.0), 1.0);
        assert_eq!(text_scale(100.0), 0.9);
        assert_eq!(text_scale(5000.0), 1.3);
        assert_eq!(text_scale(f32::NAN), 1.0);
    }

    #[test]
    fn the_wizard_takes_a_share_of_a_large_window_at_its_aspect() {
        let size = WIZARD.size(Vec2::new(1440.0, 900.0));
        assert_eq!(size, Vec2::new(1008.0, 630.0));
        assert!((size.x / size.y - 1.6).abs() < 0.01);
    }

    #[test]
    fn the_maximum_caps_a_very_large_window() {
        assert_eq!(WIZARD.size(Vec2::new(3840.0, 2160.0)), WIZARD.max);
        assert_eq!(ABOUT.size(Vec2::new(3840.0, 2160.0)), ABOUT.max);
    }

    #[test]
    fn the_minimum_holds_while_the_window_has_room() {
        assert_eq!(WIZARD.size(Vec2::new(700.0, 600.0)), WIZARD.min);
    }

    #[test]
    fn a_small_window_shrinks_the_size_and_keeps_the_aspect() {
        let size = WIZARD.size(Vec2::new(500.0, 300.0));
        assert!(size.x <= 500.0 - 2.0 * WIZARD.margin);
        assert!(size.y <= 300.0 - 2.0 * WIZARD.margin);
        assert!((size.x / size.y - 1.6).abs() < 0.02);
    }

    #[test]
    fn a_short_window_limits_the_height_first() {
        let size = ABOUT.size(Vec2::new(1600.0, 500.0));
        assert_eq!(size.y, 500.0 - 2.0 * ABOUT.margin);
        assert!((size.x / size.y - 1.6).abs() < 0.01);
    }

    #[test]
    fn a_degenerate_screen_gives_the_minimum() {
        for screen in [
            Vec2::ZERO,
            Vec2::new(-5.0, 100.0),
            Vec2::new(f32::NAN, 100.0),
            Vec2::new(100.0, f32::INFINITY),
        ] {
            assert_eq!(WIZARD.size(screen), WIZARD.min, "{screen:?}");
        }
    }

    #[test]
    fn a_bad_aspect_falls_back_to_square() {
        let rule = SizeRule {
            aspect: 0.0,
            ..WIZARD
        };
        let size = rule.size(Vec2::new(1440.0, 900.0));
        assert_eq!(size.x, size.y);
    }

    #[test]
    fn the_rect_is_centred() {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1024.0, 768.0));
        let rect = WIZARD.rect(screen);
        assert_eq!(rect.center(), screen.center());
        assert!(screen.contains_rect(rect));
    }
}
