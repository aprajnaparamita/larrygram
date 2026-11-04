//! Scene graph, PNG and SVG rendering (ADR 0020, ADR 0025).
//!
//! `build_scene` turns one page of a `larrygram-model` document into a
//! [`Scene`] through the inheritance lookup (ADR 0021), and
//! `render_png`/`render_svg` rasterize or write that scene. The scene is
//! shared with the UI (ADR 0002: `render` is L3).
//!
//! Part of Larrygram. Alpha.

#![forbid(unsafe_code)]

pub mod build;
mod error;
pub mod hit;
pub mod overlay;
pub mod png;
pub mod raster;
pub mod scene;
pub mod styles;
pub mod svg;

pub use build::{BuildOptions, FALLBACK_PAGE_SIZE, build_scene};
pub use error::Error;
pub use hit::{
    Hit, HitKind, HitOptions, NodePath, hit_marquee, hit_test, node_bounds, scene_bounds,
};
pub use overlay::{EndMark, GlueFeedback, Overlay, OverlayMark, build_selection_overlay};
pub use png::{PngOptions, render_png};
pub use raster::{RasterImage, RasterOptions, render_raster};
pub use scene::{Rgba, Scene};
pub use svg::{SvgOptions, render_svg};

/// The largest device extent of one page side, in pixels (ADR 0025
/// amendment, P3-16): both backends cap `page size x scale` at this per
/// side. A `scale` of more than this many pixels per inch cannot fit any
/// page of one inch and is [`Error::InvalidOptions`]; a page that does not
/// fit the cap at the requested scale is [`Error::TooLarge`].
pub const MAX_DEVICE_EXTENT: u32 = 16_384;

/// The scale policy shared by both backends (ADR 0025 amendment): a scale
/// that is not finite, not positive, or above [`MAX_DEVICE_EXTENT`] pixels
/// per inch returns [`Error::InvalidOptions`].
pub(crate) fn checked_scale(scale: f64) -> Result<f64, Error> {
    if !scale.is_finite() || scale <= 0.0 || scale > f64::from(MAX_DEVICE_EXTENT) {
        return Err(Error::InvalidOptions);
    }
    Ok(scale)
}
