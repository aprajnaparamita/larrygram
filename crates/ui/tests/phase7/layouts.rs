//! The four layouts at run time (task P7-09 Goal 1 and Goal 2).
//!
//! KICKOFF's exit criterion: switching between the four layouts needs
//! no restart. The tests switch through all four and back, in both
//! directions, on one `Shell` and one egui `Context`, and check that
//! each frame renders and draws the widgets of its own layout.

use egui::accesskit::Role;
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, sets, settings_for};

/// The four kinds in switch order, ending on the start kind.
const ROUND: &[LayoutKind] = &[
    LayoutKind::PanelModern,
    LayoutKind::PanelClassic,
    LayoutKind::Panel2016,
    LayoutKind::Panel365,
    LayoutKind::PanelModern,
    LayoutKind::Panel365,
    LayoutKind::Panel2016,
    LayoutKind::PanelClassic,
    LayoutKind::PanelModern,
];

/// The registry id of the layout switch (ADR 0032).
fn switch_id(kind: LayoutKind) -> &'static str {
    match kind {
        LayoutKind::PanelModern => "view.ui_layout.panel_modern",
        LayoutKind::PanelClassic => "view.ui_layout.panel_classic",
        LayoutKind::Panel2016 => "view.ui_layout.panel_2016",
        LayoutKind::Panel365 => "view.ui_layout.panel_365",
    }
}

/// One widget name only this layout draws.
fn marker(kind: LayoutKind) -> String {
    match kind {
        // The classic Shapes window header (docs/ui/panel-classic.md).
        LayoutKind::PanelClassic => "Search for Shapes:".to_owned(),
        // The Panel Modern command bar's collapse button
        // (docs/ui/panel-modern.md); the Phase 4 frame has none.
        LayoutKind::PanelModern => "Collapse the command bar".to_owned(),
        // The two Fluent-data kinds draw the v1 quick access toolbar;
        // its placements are the v1 data's own labels.
        LayoutKind::Panel2016 => sets().panel_2016.layout.quick_access_toolbar.controls[0]
            .label
            .clone(),
        LayoutKind::Panel365 => sets().panel_365.layout.quick_access_toolbar.controls[0]
            .label
            .clone(),
    }
}

#[test]
fn switching_through_all_four_layouts_and_back_renders_every_frame() {
    let mut rig = ShellRig::new(settings_for(LayoutKind::PanelModern));
    let mut previous = LayoutKind::PanelModern;
    for &kind in ROUND {
        // The switch is one registry command (ADR 0032): the same
        // path serves the dialog, the menu and the CLI.
        rig.dispatch(switch_id(kind));
        let names = rig.names();
        {
            let shell = rig.shell.borrow();
            assert_eq!(shell.layout_kind(), kind, "{previous:?} -> {kind:?}");
            assert_eq!(
                shell.app.settings.default_layout, kind,
                "the default layout stays in step (Goal 6)"
            );
        }
        let marker = marker(kind);
        assert!(
            names.contains_key(&marker),
            "{kind:?} draws its own widgets ({marker:?}): {names:?}"
        );
        previous = kind;
    }
}

#[test]
fn the_switch_works_in_both_directions_on_the_same_shell() {
    // Forward and backward between two kinds that share no widget
    // set (Panel Modern <-> Panel Classic), the same `Shell` and egui
    // `Context` throughout (no restart).
    let mut rig = ShellRig::new(settings_for(LayoutKind::PanelModern));
    for _ in 0..2 {
        rig.dispatch("view.ui_layout.panel_classic");
        assert_eq!(rig.shell.borrow().layout_kind(), LayoutKind::PanelClassic);
        assert!(rig.has("Search for Shapes:"), "the classic frame draws");
        rig.dispatch("view.ui_layout.panel_modern");
        assert_eq!(rig.shell.borrow().layout_kind(), LayoutKind::PanelModern);
        assert!(
            rig.has(&marker(LayoutKind::PanelModern)),
            "the modern frame draws"
        );
    }
}

#[test]
fn each_layout_draws_the_widgets_of_its_own_data() {
    for kind in [
        LayoutKind::PanelModern,
        LayoutKind::PanelClassic,
        LayoutKind::Panel2016,
        LayoutKind::Panel365,
    ] {
        let mut rig = ShellRig::new(settings_for(kind));
        let names = rig.names();
        let marker = marker(kind);
        assert!(
            names.contains_key(&marker),
            "{kind:?} draws {marker:?}: {names:?}"
        );
    }
}

#[test]
fn the_options_dialog_switches_the_layout_at_run_time() {
    let mut rig = ShellRig::new(settings_for(LayoutKind::PanelModern));
    rig.dispatch("app.office.center.options");
    rig.run();
    rig.click(Role::RadioButton, "Panel Classic");
    rig.run();
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.layout_kind(), LayoutKind::PanelClassic);
    }
    // No restart: the next frame draws the Panel Classic chrome.
    assert!(rig.has("Search for Shapes:"));
}
