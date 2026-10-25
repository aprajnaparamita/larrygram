//! Render-to-image for a remote caller (ADR 0079; task P9-05).
//!
//! Builds the page's scene with `larrygram_render::build_scene` and
//! encodes it as PNG or SVG, the whole page or one region. The image
//! size is computed and checked against the [`Limits`] before any pixel
//! is allocated. Writing to a file and base64 for the wire are the
//! executor's job; this module returns bytes.

use larrygram_model::{Document, PageId};
use larrygram_render::Rgba;

use crate::error::ControlError;
use crate::limits::Limits;
use crate::ops::{Region, Render, RenderFormat};

/// A rendered image (ADR 0079).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// PNG or SVG.
    pub format: RenderFormat,
    /// Width in pixels (SVG: the viewport at the scale).
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The encoded image.
    pub bytes: Vec<u8>,
}

/// The default scale, pixels per inch (ADR 0025).
pub const DEFAULT_SCALE: f64 = 96.0;

/// The page background a `render/page` call asks for (ADR 0026
/// spelling).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Background {
    /// The page's own background.
    Page,
    /// `"none"`: transparent.
    Transparent,
    /// `"#RRGGBB"`: an opaque colour.
    Color(Rgba),
}

/// Parses the `background` param (task P9-05): absent is
/// [`Background::Page`], `"none"` transparent, `"#RRGGBB"` a colour,
/// anything else `InvalidParams`.
pub fn parse_background(text: Option<&str>) -> Result<Background, ControlError> {
    let _ = text;
    Err(ControlError::not_implemented("render::parse_background"))
}

/// The pixel size of the image for a page of `page_width` by
/// `page_height` inches, the optional `region` and `scale` (task
/// P9-05). Checks the scale (finite, positive), the region (inside the
/// page, positive size) and the caps `max_render_side` and
/// `max_render_pixels` before anything is allocated.
pub fn image_size(
    page_width: f64,
    page_height: f64,
    region: Option<&Region>,
    scale: f64,
    limits: &Limits,
) -> Result<(u32, u32), ControlError> {
    let _ = (page_width, page_height, region, scale, limits);
    Err(ControlError::not_implemented("render::image_size"))
}

/// Renders `page` of `document` as `params` asks (task P9-05). The
/// `document`, `page`, `output` and `overwrite` params are the
/// executor's; this function reads `region`, `scale`, `format` and
/// `background`.
pub fn render_page(
    document: &Document,
    page: PageId,
    params: &Render,
    limits: &Limits,
) -> Result<Image, ControlError> {
    let _ = (document, page, params, limits);
    Err(ControlError::not_implemented("render::render_page"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn stub(error: ControlError) {
        assert_eq!(error.code(), ErrorCode::NotImplemented);
    }

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        stub(parse_background(None).expect_err("stub"));
        stub(image_size(8.5, 11.0, None, DEFAULT_SCALE, &limits).expect_err("stub"));
        let document = Document::new();
        let page = PageId { index: 0 };
        stub(render_page(&document, page, &Render::default(), &limits).expect_err("stub"));
        let image = Image {
            format: RenderFormat::Png,
            width: 1,
            height: 1,
            bytes: Vec::new(),
        };
        assert_eq!(image.clone(), image);
        assert_ne!(Background::Page, Background::Transparent);
    }
}
