//! The welcome wizard on the first launch and the Help menu in every
//! layout (task P7-09 Goal 5, ADR 0066).

use egui::accesskit::Role;
use larrygram_commands::Settings;
use larrygram_ui_spec::LayoutKind;

use crate::{ShellRig, settings_for};

/// The wizard's page-1 title (ADR 0066, verbatim).
const WIZARD_TITLE: &str = "Larrygram v1.0 alpha";

/// The About window's dedication line (ADR 0066, verbatim).
const DEDICATION: &str = "This app is dedicated to Larry Jeffus.";

#[test]
fn a_fresh_install_opens_the_wizard_and_start_applies_the_layout() {
    // A fresh install: no store, so `welcome_seen` is `false` and the
    // wizard opens before any document (ADR 0066).
    let mut rig = ShellRig::new(Settings::default());
    let names = rig.names();
    assert!(
        names.contains_key(WIZARD_TITLE),
        "the first launch shows the wizard: {names:?}"
    );
    rig.click(Role::Button, "Next");
    rig.run();
    // Page 2: choose Panel Classic and start.
    rig.click(Role::Button, "Panel Classic");
    rig.run();
    rig.click(Role::Button, "Start");
    rig.run();
    {
        let shell = rig.shell.borrow();
        assert_eq!(
            shell.layout_kind(),
            LayoutKind::PanelClassic,
            "Start applies the chosen layout at once (no restart)"
        );
        assert_eq!(shell.app.settings.default_layout, LayoutKind::PanelClassic);
        assert!(shell.app.settings.welcome_seen, "the wizard is seen");
    }
    // The same shell draws the Panel Classic frame on the next frame.
    assert!(rig.has("Search for Shapes:"));
}

#[test]
fn the_next_launch_skips_the_wizard() {
    // First launch: Cancel (which also marks the wizard as seen).
    let mut rig = ShellRig::new(Settings::default());
    rig.click(Role::Button, "Cancel");
    rig.run();
    {
        let shell = rig.shell.borrow();
        assert!(shell.app.settings.welcome_seen);
    }
    let settings = rig.shell.borrow().app.settings.clone();
    // The next launch over the stored settings: no wizard.
    let mut rig = ShellRig::new(settings);
    let names = rig.names();
    assert!(
        !names.contains_key(WIZARD_TITLE),
        "the wizard does not open again: {names:?}"
    );
}

#[test]
fn cancel_keeps_the_current_layout() {
    // A store that already named Panel (2016 data) keeps it on
    // Cancel (task P7-09 Goal 2: stored values are kept).
    let mut settings = Settings::default();
    settings.ui_layout = LayoutKind::Panel2016;
    let mut rig = ShellRig::new(settings);
    rig.click(Role::Button, "Cancel");
    rig.run();
    let shell = rig.shell.borrow();
    assert_eq!(shell.layout_kind(), LayoutKind::Panel2016);
    assert!(shell.app.settings.welcome_seen);
}

#[test]
fn the_options_row_shows_the_wizard_on_the_next_start() {
    let mut rig = ShellRig::new(settings_for(LayoutKind::PanelModern));
    rig.dispatch("app.office.center.options");
    rig.run();
    rig.click(Role::CheckBox, "Show the welcome wizard on next start");
    rig.run();
    {
        let shell = rig.shell.borrow();
        assert!(
            !shell.app.settings.welcome_seen,
            "the row clears `welcome_seen` (ADR 0066)"
        );
    }
    // Not this session: it opens on the next start.
    assert!(!rig.has(WIZARD_TITLE));
    let settings = rig.shell.borrow().app.settings.clone();
    let mut next = ShellRig::new(settings);
    assert!(next.has(WIZARD_TITLE), "the next start opens the wizard");
}

#[test]
fn help_about_and_help_welcome_work_in_every_layout() {
    for kind in [
        LayoutKind::PanelModern,
        LayoutKind::PanelClassic,
        LayoutKind::Panel2016,
        LayoutKind::Panel365,
    ] {
        let mut rig = ShellRig::new(settings_for(kind));
        // Help > About Larrygram: the About window (ADR 0066).
        rig.dispatch("help.about");
        let names = rig.names();
        assert!(
            names.contains_key(DEDICATION),
            "{kind:?} opens the About window: {names:?}"
        );
        rig.click(Role::Button, "Close");
        rig.run();
        // Help > Welcome: the wizard again.
        rig.dispatch("help.welcome");
        let names = rig.names();
        assert!(
            names.contains_key(WIZARD_TITLE),
            "{kind:?} opens the wizard: {names:?}"
        );
    }
}
