//! The `.ico` builder shared by `build.rs` and the tests (task R11
//! goal 3).
//!
//! `build.rs` includes this file (`#[path]`) to build the Windows
//! icon resource at build time, and `tests/build_support.rs` includes
//! the same file so the container format is unit tested (a build
//! script's own tests never run). No code here is committed as an
//! asset: the `.ico` is derived from the brand `app-icon.svg` at build
//! time (task R11 master notes).

/// The icon sizes in the `.ico`, in pixels (task R11 goal 3).
pub const ICON_SIZES: [u32; 6] = [16, 32, 48, 64, 128, 256];

/// Rasterises one SVG into a multi-size `.ico` of PNG images (task
/// R11 goal 3). PNG entries are what Vista and later read. A broken
/// SVG or a failing raster is an error, never a panic (AGENTS.md
/// section 6).
pub fn build_ico(svg: &str) -> Result<Vec<u8>, String> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
        .map_err(|error| format!("cannot parse app-icon.svg: {error}"))?;
    let width = tree.size().width().max(1.0);
    let height = tree.size().height().max(1.0);
    let mut images = Vec::with_capacity(ICON_SIZES.len());
    for size in ICON_SIZES {
        let mut pixmap =
            resvg::tiny_skia::Pixmap::new(size, size).ok_or("cannot allocate a pixmap")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(size as f32 / width, size as f32 / height),
            &mut pixmap.as_mut(),
        );
        let png = pixmap
            .encode_png()
            .map_err(|error| format!("cannot encode the {size} px icon: {error}"))?;
        images.push((size, png));
    }
    Ok(ico_container(&images))
}

/// Packs PNG images into one `.ico` container (task R11 goal 3): the
/// ICONDIR header, one ICONDIRENTRY per image, then the image data.
pub fn ico_container(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved, must be 0
    out.extend_from_slice(&1u16.to_le_bytes()); // type 1: icon
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = u32::from(count) * 16 + 6;
    for (size, png) in images {
        // A 256 px side is stored as 0 (task R11 goal 3).
        let dim = if *size >= 256 {
            0u8
        } else {
            u8::try_from(*size).unwrap_or(0)
        };
        out.push(dim); // width
        out.push(dim); // height
        out.push(0); // palette colours, 0: no palette
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_container_holds_every_image() {
        let images = vec![(16u32, vec![1u8, 2, 3]), (256u32, vec![4u8, 5])];
        let ico = ico_container(&images);
        assert_eq!(&ico[0..2], &0u16.to_le_bytes(), "reserved");
        assert_eq!(&ico[2..4], &1u16.to_le_bytes(), "type: icon");
        assert_eq!(&ico[4..6], &2u16.to_le_bytes(), "two images");
        // One entry per image, 16 bytes each, then the data.
        assert_eq!(ico.len(), 6 + 16 * 2 + 5);
        assert_eq!(ico[6], 16, "the 16 px side");
        assert_eq!(ico[7], 16);
        assert_eq!(ico[6 + 16], 0, "a 256 px side is stored as 0");
        assert_eq!(ico[6 + 16 + 1], 0);
        // The first image's bytes are at the first entry's offset.
        let offset = u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]) as usize;
        assert_eq!(offset, 6 + 16 * 2);
        assert_eq!(&ico[offset..offset + 3], &[1, 2, 3]);
    }

    #[test]
    fn a_broken_svg_is_an_error_not_a_panic() {
        assert!(build_ico("this is not svg").is_err());
    }

    #[test]
    fn the_brand_icon_rasterises_into_a_container() {
        let svg = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("crates")
                .join("ui")
                .join("assets")
                .join("brand")
                .join("app-icon.svg"),
        )
        .expect("the brand app-icon.svg reads");
        let ico = build_ico(&svg).expect("the brand icon rasterises");
        assert_eq!(&ico[4..6], &(ICON_SIZES.len() as u16).to_le_bytes());
    }
}
