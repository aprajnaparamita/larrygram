//! Layout switching without a restart (task P4-09, ADR 0032).
//!
//! KICKOFF's exit criterion: "both layouts load and switch without a
//! restart". The test starts with the 2016 Panel layout, dispatches
//! `view.ui_layout.panel_365`, and shows that the *same* frame series
//! (same egui `Context`, same `AppCtx`, same `Shell`) draws the 365
//! tabs on the next frame: no restart, no widget rebuild from
//! scratch. The switch is one registry command (ADR 0032), so the
//! same path serves the dialog, the menu and (later) the JSON
//! channel.

use std::collections::BTreeSet;

use larrygram_commands::Source;
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, sets};

/// The command-bar tab labels of one layout.
fn tab_labels(kind: LayoutKind) -> BTreeSet<String> {
    sets()
        .get(kind)
        .layout
        .tabs
        .iter()
        .map(|tab| tab.label.clone())
        .collect()
}

/// A label that only one of the two layouts has (the 2016 tab set is
/// a subset of the 365 one in the extracted data, so the marker is
/// always on the 365 side).
fn unique_tab(kind: LayoutKind, other: LayoutKind) -> Option<String> {
    let mine = tab_labels(kind);
    let theirs = tab_labels(other);
    mine.difference(&theirs).next().cloned()
}

#[test]
fn switching_the_panel_layout_needs_no_restart() {
    let mut rig = ShellRig::new();
    let shell = rig.shell.borrow();
    assert_eq!(shell.layout_kind(), LayoutKind::Panel2016);
    drop(shell);

    // State that must survive the switch: an open document, a
    // warning, and egui's own memory (a restart would lose all
    // three).
    rig.dispatch("file.new", Source::Ui).expect("file.new runs");
    {
        let mut shell = rig.shell.borrow_mut();
        shell.note_error("p4-09 marker warning");
    }
    let ctx = rig
        .egui_ctx
        .borrow()
        .clone()
        .expect("the harness context was captured");
    let marker = egui::Id::unique("p4-09-layout-switch");
    ctx.data_mut(|data| data.insert_temp(marker, 42u32));
    let app_ptr: usize = {
        let shell = rig.shell.borrow();
        &shell.app as *const _ as usize
    };

    // A tab only the 365 layout has: the switch marker.
    let only_365 = unique_tab(LayoutKind::Panel365, LayoutKind::Panel2016)
        .expect("365 has a tab 2016 does not");
    let shared_2016 = unique_tab(LayoutKind::Panel2016, LayoutKind::Panel365)
        .or_else(|| tab_labels(LayoutKind::Panel2016).into_iter().next())
        .expect("2016 has tabs");

    // The first frame shows the 2016 tabs.
    rig.run();
    let before = rig.names();
    assert!(
        before.contains_key(&shared_2016),
        "the 2016 tab {shared_2016:?} renders: {before:?}"
    );
    assert!(
        !before.contains_key(&only_365),
        "no 365-only tab before the switch"
    );

    // One command flips the setting; the next frame of the same
    // harness shows the Panel (365 data) layout.
    let outcome = rig
        .dispatch("view.ui_layout.panel_365", Source::Ui)
        .expect("the layout switch runs");
    assert!(outcome.message.is_some(), "the switch reports its message");
    rig.run();
    let after = rig.names();
    assert!(
        after.contains_key(&only_365),
        "the 365-only tab {only_365:?} renders after the switch: {after:?}"
    );
    assert!(
        after.contains_key(&shared_2016),
        "the shared tabs keep rendering after the switch"
    );

    // Nothing restarted: the same shell, the same AppCtx, the same
    // egui Context (its memory survived) and the open document too.
    {
        let shell = rig.shell.borrow();
        assert_eq!(shell.layout_kind(), LayoutKind::Panel365);
        assert_eq!(
            &shell.app as *const _ as usize, app_ptr,
            "the AppCtx instance did not change"
        );
        assert!(
            shell
                .app
                .warnings
                .iter()
                .any(|w| w.contains("p4-09 marker warning")),
            "the recorded warning survived the switch"
        );
        assert!(
            shell.app.active_document().is_some(),
            "the open document survived the switch"
        );
    }
    assert_eq!(
        ctx.data(|data| data.get_temp::<u32>(marker)),
        Some(42),
        "the same egui Context is still running"
    );

    // And back again: the 365-only tab leaves with the layout.
    rig.dispatch("view.ui_layout.panel_2016", Source::Ui)
        .expect("the switch back runs");
    rig.run();
    let back = rig.names();
    assert!(back.contains_key(&shared_2016), "the 2016 tabs return");
    assert!(!back.contains_key(&only_365));
}

#[test]
fn both_layouts_load_and_render_their_own_tabs() {
    // The other half of the criterion: each layout loads from the
    // embedded data and renders its own tab set.
    for kind in [LayoutKind::Panel2016, LayoutKind::Panel365] {
        let mut rig = ShellRig::new();
        rig.dispatch(
            match kind {
                LayoutKind::Panel2016 => "view.ui_layout.panel_2016",
                LayoutKind::Panel365 => "view.ui_layout.panel_365",
                other => panic!("not a Fluent-data layout: {other:?}"),
            },
            Source::Ui,
        )
        .expect("the layout switch runs");
        rig.run();
        let names = rig.names();
        for label in tab_labels(kind) {
            assert!(
                names.contains_key(&label),
                "{kind:?} tab {label:?} renders: {names:?}"
            );
        }
    }
}
