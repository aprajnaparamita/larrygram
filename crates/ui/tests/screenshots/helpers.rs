//! The screenshot helpers (task P7-38 goal 2): exact-aspect crops,
//! minimum sizes and the 3x fallback. The gate runs these ordinary
//! unit tests on synthetic images, so the crop and size rules stay
//! pinned even though the shot renders themselves are `#[ignore]`d.
//!
//! The rules every shot follows:
//!
//! - a crop has the shot's exact aspect (the pixel size is an integer
//!   multiple of the reduced aspect pair, so 16:10 is exactly 8:5);
//! - every PNG is at least the shot's minimum size;
//! - a crop that falls short of the minimum at 2x re-renders the shot
//!   at 3x instead of upscaling.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use image::RgbaImage;

/// One rendered frame and the crop rectangle the shot wants (logical
/// window pixels; the crop keeps the shot's aspect).
pub struct Frame {
    /// The full window as the wgpu renderer drew it.
    pub image: RgbaImage,
    /// The crop rectangle in logical window pixels.
    pub crop: egui::Rect,
}

/// The greatest common divisor (Euclid).
fn gcd(a: u32, b: u32) -> u32 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The largest exact-`aspect` pixel size that fits in `size`: an
/// integer multiple of the reduced aspect pair (so the ratio holds
/// exactly), never larger than `size` in either dimension.
pub fn fit_aspect_px(size: (u32, u32), aspect: (u32, u32)) -> (u32, u32) {
    let g = gcd(aspect.0, aspect.1).max(1);
    let (a, b) = (aspect.0 / g, aspect.1 / g);
    let k = (size.0 / a).min(size.1 / b);
    (k * a, k * b)
}

/// A crop rectangle of exact `aspect` centred on `focus`, at least
/// `min_px` pixels at `scale`, and as close to `focus` as `bounds`
/// allows (never smaller than the minimum: a shot that cannot fit is
/// a failed capture, not a shrunken crop).
pub fn crop_around(
    focus: egui::Rect,
    bounds: egui::Rect,
    aspect: (u32, u32),
    min_px: (u32, u32),
    scale: f32,
) -> egui::Rect {
    let ratio = f64::from(aspect.0) / f64::from(aspect.1);
    // The height that covers `focus` at the aspect, then the height
    // that covers the minimum (both dimensions).
    let mut height = focus.height().max(focus.width() / ratio as f32);
    let min_w = min_px.0 as f32 / scale;
    let min_h = min_px.1 as f32 / scale;
    height = height.max(min_h).max(min_w / ratio as f32);
    // A couple of logical pixels of slack absorb the pixel rounding at
    // crop time (the exact-aspect fit shrinks a pixel or two).
    height += 2.0;
    let width = height * ratio as f32;
    let size = egui::vec2(width, height);
    // Centre on `focus`, keeping the rectangle inside `bounds` (when
    // the crop is bigger than `bounds`, it stays centred).
    let mut center = focus.center();
    let half = size / 2.0;
    for (value, min, max) in [
        (&mut center.x, bounds.min.x + half.x, bounds.max.x - half.x),
        (&mut center.y, bounds.min.y + half.y, bounds.max.y - half.y),
    ] {
        if min <= max {
            *value = value.clamp(min, max);
        } else {
            *value = (min + max) / 2.0;
        }
    }
    egui::Rect::from_center_size(center, size)
}

/// A pixel rectangle in a rendered image: `x`, `y`, `width`,
/// `height`.
type PxRect = (u32, u32, u32, u32);

/// The frame's logical crop at `scale`, in image pixels, intersected
/// with the image: what the frame can really give (a crop rectangle
/// larger than the window is cut back to the window, never padded).
fn available_px(frame: &Frame, scale: f32) -> PxRect {
    let (image_w, image_h) = frame.image.dimensions();
    let edge = |value: f32, max: u32| (value * scale).round().clamp(0.0, max as f32) as u32;
    let (x0, x1) = (
        edge(frame.crop.min.x, image_w),
        edge(frame.crop.max.x, image_w),
    );
    let (y0, y1) = (
        edge(frame.crop.min.y, image_h),
        edge(frame.crop.max.y, image_h),
    );
    (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

/// Crops `image` to `size_px` (already the exact aspect, no larger
/// than `area`) centred in `area`.
fn crop_image(image: &RgbaImage, area: PxRect, size_px: (u32, u32)) -> RgbaImage {
    let (x0, y0, area_w, area_h) = area;
    let (width, height) = size_px;
    let x = x0 + (area_w - width) / 2;
    let y = y0 + (area_h - height) / 2;
    image::imageops::crop_imm(image, x, y, width, height).to_image()
}

/// Renders one shot through `build` and crops it to `aspect` at least
/// `min_px` in size. The first attempt renders at 2x; when the crop
/// the frame can really give (its rectangle cut to the image) falls
/// short of the minimum, the shot re-renders at 3x (never upscale).
/// `build` receives the scale and returns the full frame with its
/// logical crop rectangle.
pub fn capture(
    mut build: impl FnMut(f32) -> Frame,
    aspect: (u32, u32),
    min_px: (u32, u32),
) -> RgbaImage {
    let mut short = String::new();
    for scale in [2.0f32, 3.0] {
        let frame = build(scale);
        let area = available_px(&frame, scale);
        let (w, h) = fit_aspect_px((area.2, area.3), aspect);
        if w >= min_px.0 && h >= min_px.1 {
            return crop_image(&frame.image, area, (w, h));
        }
        short = format!("at {scale}x the crop is {w}x{h}");
    }
    panic!(
        "{short}, below the minimum {}x{}; \
         the shot needs a larger crop or a larger window",
        min_px.0, min_px.1
    )
}

// --- the rules' own tests (synthetic images) ------------------------

/// A synthetic `scale`-rendered window (solid grey) with a crop
/// rectangle.
fn synthetic(window: (f32, f32), crop: egui::Rect, scale: f32) -> Frame {
    Frame {
        image: RgbaImage::from_pixel(
            (window.0 * scale) as u32,
            (window.1 * scale) as u32,
            image::Rgba([128, 128, 128, 255]),
        ),
        crop,
    }
}

/// A rectangle from its logical min and size.
fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

#[test]
fn fit_aspect_is_exact_and_largest() {
    // 16:10 reduces to 8:5; the fit is the largest multiple inside.
    assert_eq!(fit_aspect_px((2000, 1200), (16, 10)), (1920, 1200));
    assert_eq!(fit_aspect_px((3200, 2000), (16, 10)), (3200, 2000));
    assert_eq!(fit_aspect_px((101, 100), (4, 3)), (100, 75));
    assert_eq!(fit_aspect_px((100, 100), (4, 3)), (100, 75));
    assert_eq!(fit_aspect_px((1602, 1202), (4, 3)), (1600, 1200));
    // Too small for one aspect unit: nothing fits.
    assert_eq!(fit_aspect_px((3, 100), (4, 3)), (0, 0));
}

#[test]
fn fit_aspect_ratios_hold_exactly() {
    for (w, h) in [
        fit_aspect_px((2000, 1200), (16, 10)),
        fit_aspect_px((1234, 567), (4, 3)),
        fit_aspect_px((1920, 1080), (16, 9)),
    ] {
        assert!(
            u64::from(w) * 10 == u64::from(h) * 16
                || u64::from(w) * 3 == u64::from(h) * 4
                || u64::from(w) * 9 == u64::from(h) * 16,
            "{w}x{h} is one of the three exact ratios"
        );
    }
}

#[test]
fn capture_keeps_the_window_at_2x_when_the_minimum_is_met() {
    let mut scales = Vec::new();
    let image = capture(
        |scale| {
            scales.push(scale);
            synthetic((1600.0, 1000.0), rect(0.0, 0.0, 1600.0, 1000.0), scale)
        },
        (16, 10),
        (2560, 1600),
    );
    assert_eq!(scales, vec![2.0], "one render at 2x");
    assert_eq!(image.dimensions(), (3200, 2000));
}

#[test]
fn capture_falls_back_to_3x_when_the_crop_is_short() {
    let mut scales = Vec::new();
    // A 500 x 400 crop: 1000 x 800 at 2x misses 1600 x 1200, so the
    // shot re-renders at 3x (1500 x 1200 is still short in width...
    // widen to 700 x 500: 2100 x 1500 at 3x passes).
    let image = capture(
        |scale| {
            scales.push(scale);
            synthetic((1600.0, 1000.0), rect(100.0, 100.0, 700.0, 500.0), scale)
        },
        (4, 3),
        (1600, 1200),
    );
    assert_eq!(scales, vec![2.0, 3.0], "2x first, then 3x");
    // The exact-aspect fit of 2100 x 1500 at 4:3.
    assert_eq!(image.dimensions(), (2000, 1500));
}

#[test]
fn capture_output_has_the_exact_aspect() {
    let image = capture(
        |scale| {
            // A crop that rounds off-aspect.
            synthetic((1600.0, 1000.0), rect(0.0, 0.0, 801.0, 601.0), scale)
        },
        (4, 3),
        (800, 600),
    );
    let (w, h) = image.dimensions();
    assert_eq!(u64::from(w) * 3, u64::from(h) * 4, "{w}x{h} is 4:3");
    assert!(w >= 800 && h >= 600, "{w}x{h} meets the minimum");
}

#[test]
fn capture_fails_with_a_clear_message_when_short_even_at_3x() {
    let message = std::panic::catch_unwind(|| {
        capture(
            |scale| synthetic((100.0, 100.0), rect(0.0, 0.0, 100.0, 100.0), scale),
            (4, 3),
            (1600, 1200),
        )
    })
    .expect_err("the minimum cannot be met");
    let message = message
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    assert!(message.contains("below the minimum 1600x1200"), "{message}");
}

#[test]
fn crop_around_covers_the_focus_and_the_minimum() {
    let focus = rect(700.0, 400.0, 200.0, 150.0);
    let bounds = rect(0.0, 0.0, 1600.0, 1000.0);
    let crop = crop_around(focus, bounds, (4, 3), (1600, 1200), 2.0);
    assert!(
        crop.contains_rect(focus),
        "{crop:?} covers the focus {focus:?}"
    );
    assert!(
        crop.width() * 2.0 >= 1600.0 && crop.height() * 2.0 >= 1200.0,
        "{crop:?} meets the minimum at 2x"
    );
    let (w, h) = (
        (crop.width() * 2.0).round() as u32,
        (crop.height() * 2.0).round() as u32,
    );
    let (w, h) = fit_aspect_px((w, h), (4, 3));
    assert!(
        w >= 1600 && h >= 1200,
        "the fitted crop stays at least the minimum: {w}x{h}"
    );
}

#[test]
fn crop_around_keeps_the_aspect() {
    let focus = rect(600.0, 300.0, 300.0, 100.0);
    let bounds = rect(0.0, 0.0, 1600.0, 1000.0);
    let crop = crop_around(focus, bounds, (16, 9), (320, 180), 1.0);
    let ratio = crop.width() / crop.height();
    assert!((ratio - 16.0 / 9.0).abs() < 1e-4, "{crop:?} keeps 16:9");
}

#[test]
fn crop_around_stays_inside_the_bounds() {
    // A focus near the corner: the crop recentres into the window
    // instead of cropping outside the image.
    let focus = rect(5.0, 5.0, 100.0, 80.0);
    let bounds = rect(0.0, 0.0, 800.0, 600.0);
    let crop = crop_around(focus, bounds, (4, 3), (400, 300), 1.0);
    assert!(
        bounds.contains_rect(crop),
        "{crop:?} stays inside {bounds:?}"
    );
}

#[test]
fn crop_around_recentres_a_crop_bigger_than_the_bounds() {
    // The crop cannot fit; it stays centred instead of panicking.
    let focus = rect(780.0, 480.0, 40.0, 40.0);
    let bounds = rect(0.0, 0.0, 800.0, 600.0);
    let crop = crop_around(focus, bounds, (4, 3), (4000, 3000), 1.0);
    assert!(
        (crop.center() - bounds.center()).length() < 0.5,
        "{crop:?} is centred in {bounds:?}"
    );
}

#[test]
fn capture_falls_back_to_3x_when_the_crop_is_larger_than_the_frame() {
    // The crop asks for more than the 700 x 525 window: at 2x the
    // image is only 1400 x 1050, below 1600 x 1200, so the shot must
    // re-render at 3x (2100 x 1575) instead of clipping silently.
    let mut scales = Vec::new();
    let image = capture(
        |scale| {
            scales.push(scale);
            synthetic((700.0, 525.0), rect(-50.0, -40.0, 900.0, 700.0), scale)
        },
        (4, 3),
        (1600, 1200),
    );
    assert_eq!(scales, vec![2.0, 3.0], "2x first, then 3x");
    assert_eq!(image.dimensions(), (2100, 1575));
}

#[test]
fn capture_measures_the_part_of_the_crop_inside_the_image() {
    // Half the crop hangs off the right edge: only the inside half
    // counts, and the output stays inside the image at the exact
    // aspect.
    let image = capture(
        |scale| synthetic((1000.0, 600.0), rect(600.0, 0.0, 800.0, 600.0), scale),
        (4, 3),
        (400, 300),
    );
    // 400 logical px inside at 2x: 800 x 1200 available, 800 x 600 fit.
    assert_eq!(image.dimensions(), (800, 600));
}
