//! Hit target to context menu, for both committed layouts (task P7-08).

use larrygram_ui::contexts::{HitTarget, menu_for};
use larrygram_ui_spec::v2::LayoutV2;

use super::{CLASSIC_YAML, MODERN_YAML};

fn layout(text: &str) -> LayoutV2 {
    serde_saphyr::from_str(text).expect("the committed layout decodes")
}

/// The triggers listed in `docs/ui/panel-classic.md`.
const DOCUMENTED: &[(HitTarget, &str)] = &[
    (HitTarget::Shape, "context.shape"),
    (HitTarget::TextEditing, "context.text_edit"),
    (HitTarget::PageTab, "context.page_tab"),
    (HitTarget::ConnectionPoint, "context.connection_point"),
    (HitTarget::CommentMark, "context.comment"),
    (HitTarget::Hyperlink, "context.hyperlink"),
    (HitTarget::WindowTitle, "context.window_title"),
    (HitTarget::ShapesWindow, "context.shapes_window"),
    (HitTarget::ShapeDataWindow, "context.shape_data_window"),
    (HitTarget::Shapesheet, "context.shapesheet"),
];

#[test]
fn classic_has_every_documented_context_menu() {
    let layout = layout(CLASSIC_YAML);
    for (hit, id) in DOCUMENTED {
        let menu = menu_for(&layout, *hit).unwrap_or_else(|| panic!("{id} is missing"));
        assert_eq!(menu.id, *id);
        assert!(menu.trigger.is_some(), "{id} has a trigger");
    }
}

#[test]
fn modern_has_the_menus_it_defines() {
    let layout = layout(MODERN_YAML);
    for (hit, id) in DOCUMENTED {
        let found = menu_for(&layout, *hit);
        let defined = layout.context_menus.iter().any(|menu| menu.id == *id);
        assert_eq!(found.is_some(), defined, "{id} in Panel Modern");
        if let Some(menu) = found {
            assert_eq!(menu.id, *id);
        }
    }
}

#[test]
fn an_undefined_target_returns_none() {
    let layout = layout(MODERN_YAML);
    assert!(menu_for(&layout, HitTarget::CommentMark).is_none());
    assert!(menu_for(&layout, HitTarget::ShapesWindow).is_none());
}
