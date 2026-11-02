//! Scratch: Selawik 12px text metrics (deleted before hand-off).
#[test]
fn measure() {
    let ctx = egui::Context::default();
    ctx.run_ui(Default::default(), |ctx| {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "selawk".to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../text/fonts/selawk.ttf"
            ))),
        );
        fonts
            .families
            .insert(egui::FontFamily::Proportional, vec!["selawk".to_owned()]);
        ctx.set_fonts(fonts);
        let style = egui::Style::default();
        let mut style = style;
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
        );
        ctx.set_style(style);
        ctx.fonts_mut(|f| {
            let id = egui::FontId::new(12.0, egui::FontFamily::Proportional);
            for s in [
                "012345678901234567890123456789",
                "This is a thirty character str",
                "Process",
                "Decision",
                "Cost",
                "A much longer value string that will not fit",
            ] {
                let w = f
                    .layout_no_wrap(s.to_owned(), id.clone(), egui::Color32::WHITE)
                    .rect
                    .width();
                eprintln!("{w:6.1}  {:?}", s);
            }
        });
    });
    panic!("see stderr");
}
