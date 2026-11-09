//! A narrow window collapses the Panel Modern groups into caption
//! buttons whose popup holds the whole group (task P7-17's "Narrow
//! windows" rule), driven through the real shell.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use larrygram_ui_spec::LayoutKind;

use crate::{RichApp, ShellRig, sets};

/// A narrow Panel Modern window: the groups that do not fit collapse
/// into one caption button each, and a collapsed button's popup draws
/// the whole group at full size.
#[test]
fn a_narrow_window_collapses_the_groups_into_buttons() {
    let mut app = RichApp::rich().0;
    app.settings.ui_layout = LayoutKind::PanelModern;
    let mut rig = ShellRig::with_app_at(app, egui::vec2(720.0, 480.0), 1.0);
    rig.run();
    // A captioned group of the active tab whose controls carry names:
    // the collapsed button shows the caption, the popup the controls.
    let layout = &sets().get(LayoutKind::PanelModern).v2;
    let mut found = None;
    'groups: for group in &layout.tabs[0].groups {
        let Some(caption) = group.label.clone() else {
            continue;
        };
        let control = larrygram_ui::panel_modern::place::visible_controls(group)
            .iter()
            .find_map(|item| item.label.clone());
        let Some(control) = control else {
            continue;
        };
        rig.run();
        {
            let root = rig.harness().root();
            if root
                .query_all_by_role_and_label(Role::Button, &caption)
                .next()
                .is_some()
            {
                found = Some((caption, control));
                break 'groups;
            }
        }
    }
    let (caption, control) = found.expect("some group collapses at a narrow width");
    {
        let root = rig.harness().root();
        let button = root
            .query_all_by_role_and_label(Role::Button, &caption)
            .next()
            .expect("the collapsed group button");
        button.click();
    }
    rig.run();
    assert!(
        rig.has(&control),
        "{caption:?}: the popup draws the group's controls ({control:?})"
    );
}
