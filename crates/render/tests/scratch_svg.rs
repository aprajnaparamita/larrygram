// Temporary debug (not for hand-off).
#![allow(missing_docs)]
use larrygram_render::{BuildOptions, SvgOptions, build_scene, render_svg};
use larrygram_vsd_binary::{Limits, import};
use larrygram_model::PageId;

#[test]
fn scratch_svg() {
    let name = std::env::var("SCRATCH_FILE").unwrap_or_else(|_| "no-bgcolor.vsd".to_owned());
    let path = format!("{}/../../corpus/vsd/{name}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap();
    let imported = import(&bytes, &Limits::DEFAULT).unwrap();
    let scene = build_scene(&imported.document, PageId { index: 0 }, &BuildOptions::new()).unwrap();
    let svg = render_svg(&scene, &SvgOptions::new()).unwrap();
    println!("{svg}");
}
