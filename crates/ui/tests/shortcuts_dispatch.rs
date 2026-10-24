//! Shortcut dispatch against a plain `egui::Context` (task P4-08,
//! amended by P4-14, ADR 0036): every entry of the committed
//! `ui-spec/shortcuts.yaml` loads, one binding per (sequence, scope)
//! pair with the most specific active scope winning, multi-chord
//! sequences and their timeout work, scopes filter by focus, and the
//! chords egui owns for text editing (typing and the caret keys) are
//! never stolen.

use egui::{Context, Event, Key, Modifiers, RawInput};
use larrygram_commands::{AppCtx, Settings};
use larrygram_ui::Intent;
use larrygram_ui::shortcuts::{SEQUENCE_TIMEOUT_SECONDS, ShortcutMap, dispatch, token_is_known};
use larrygram_ui_spec::{
    CommandId as SpecCommandId, LayoutKind, Loader, Shortcut, ShortcutScope, Shortcuts,
};

/// The committed shortcut table (embedded, same bytes as the file).
#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn committed_shortcuts() -> Shortcuts {
    let sets = Loader::from_embedded().expect("the embedded ui-spec loads");
    sets.get(LayoutKind::Panel2016)
        .shortcuts
        .clone()
}

#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn shortcut(tokens: &[&str], command: &str, scope: ShortcutScope) -> Shortcut {
    Shortcut {
        sequence: vec![tokens.iter().map(|t| (*t).to_owned()).collect()],
        command: SpecCommandId::parse(command).expect("id"),
        scope,
        section: None,
    }
}

#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn multi_shortcut(chords: &[&[&str]], command: &str, scope: ShortcutScope) -> Shortcut {
    Shortcut {
        sequence: chords
            .iter()
            .map(|chord| chord.iter().map(|t| (*t).to_owned()).collect())
            .collect(),
        command: SpecCommandId::parse(command).expect("id"),
        scope,
        section: None,
    }
}

#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn map_of(entries: Vec<Shortcut>) -> ShortcutMap {
    ShortcutMap::from_shortcuts(&Shortcuts { entries }).expect("map")
}

fn key_event(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Default::default()
    }
}

fn ctrl_shift() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Default::default()
    }
}

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Default::default()
    }
}

/// One frame: `dispatch` first (before the widgets, ADR 0036), then an
/// optional focused text edit.
struct Frame {
    /// What `dispatch` returned.
    intents: Vec<Intent>,
    /// How many input events survived `dispatch` (not consumed).
    events_left: usize,
    /// `ShortcutMap::pending` after the frame.
    pending: bool,
}

impl Frame {
    /// The first dispatched intent, when one fired.
    fn intent(&self) -> Option<&Intent> {
        self.intents.first()
    }
}

#[allow(clippy::expect_used)] // test helper: a failure here is the bug
fn frame(
    ctx: &Context,
    map: &mut ShortcutMap,
    app: &AppCtx,
    scope: ShortcutScope,
    events: Vec<Event>,
    time: f64,
    with_text_edit: bool,
) -> Frame {
    let mut text = String::from("hello");
    let mut frame = Frame {
        intents: Vec::new(),
        events_left: 0,
        pending: false,
    };
    let mut output = ctx.run_ui(
        RawInput {
            events,
            time: Some(time),
            ..Default::default()
        },
        |ctx| {
            frame.intents = dispatch(ctx, map, app, scope).expect("dispatch");
            frame.events_left = ctx.input(|input| input.events.len());
            frame.pending = map.pending();
            if with_text_edit {
                let mut edit_id = None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    let output = egui::TextEdit::singleline(&mut text).show(ui);
                    edit_id = Some(output.response.id);
                });
                if let Some(id) = edit_id {
                    ctx.memory_mut(|memory| memory.request_focus(id));
                }
            }
        },
    );
    output.textures_delta.clear();
    frame
}

#[test]
fn the_committed_shortcut_table_loads() {
    let committed = committed_shortcuts();
    assert!(!committed.entries.is_empty());
    let map = map_of(committed.entries.clone());
    assert!(!map.is_empty());
    // One binding per (sequence, scope) pair: the 90 entries of the
    // committed table collapse into 74 bindings and 16 exact repeats,
    // all dropped silently (P4-14).
    assert_eq!(map.len(), 74);
    assert_eq!(map.len() + 16, committed.entries.len());
}

#[test]
fn the_committed_table_has_no_sequence_scope_conflict() {
    // The amended conflict key (ADR 0036): only a (sequence, scope)
    // pair naming two different commands is a conflict. The committed
    // table has none (P4-14 Result).
    let map = map_of(committed_shortcuts().entries.clone());
    assert!(map.conflicts().is_empty(), "{:?}", map.conflicts());
}

#[test]
fn len_counts_the_bindings_of_the_committed_table() {
    let map = map_of(committed_shortcuts().entries.clone());
    // `len()` counts bindings, not source entries and not sequences:
    // 61 distinct sequences, 74 (sequence, scope) pairs.
    assert_eq!(map.len(), 74);
}

#[test]
fn the_token_table_covers_the_committed_file() {
    for shortcut in &committed_shortcuts().entries {
        for chord in &shortcut.sequence {
            for token in chord {
                assert!(
                    token_is_known(token),
                    "token {token:?} of {} is not in the egui table",
                    shortcut.command
                );
            }
        }
    }
}

#[test]
fn a_single_chord_shortcut_per_scope_resolves() {
    let mut map = map_of(vec![
        shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
        shortcut(&["F2"], "help.contents", ShortcutScope::Window),
        shortcut(
            &["Ctrl", "Shift", "P"],
            "view.zoom.fit",
            ShortcutScope::Page,
        ),
        shortcut(&["Alt", "R"], "file.print", ShortcutScope::CommandBar),
    ]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    let global = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::G, ctrl())],
        0.0,
        false,
    );
    let intent = global.intent().expect("a Global shortcut fires");
    assert_eq!(intent.command.as_str(), "arrange.group");
    assert_eq!(
        intent.invocation.source,
        larrygram_commands::Source::Shortcut
    );
    assert_eq!(global.events_left, 0, "the matched chord is consumed");

    let window = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::F2, Modifiers::default())],
        0.1,
        false,
    );
    assert_eq!(
        window
            .intent()
            .expect("a Window shortcut fires")
            .command
            .as_str(),
        "help.contents"
    );

    let page = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Page,
        vec![key_event(Key::P, ctrl_shift())],
        0.2,
        false,
    );
    assert_eq!(
        page.intent()
            .expect("a Page shortcut fires")
            .command
            .as_str(),
        "view.zoom.fit"
    );

    // Page scope off: not dispatched (and the chord is consumed).
    let no_page = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::P, ctrl_shift())],
        0.3,
        false,
    );
    assert!(no_page.intents.is_empty());
    assert_eq!(no_page.events_left, 0);

    let command_bar = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::CommandBar,
        vec![key_event(Key::R, alt())],
        0.4,
        false,
    );
    assert_eq!(
        command_bar
            .intent()
            .expect("a CommandBar shortcut fires")
            .command
            .as_str(),
        "file.print"
    );
}

#[test]
fn a_text_shortcut_fires_only_with_a_focused_text_edit() {
    let mut map = map_of(vec![shortcut(
        &["Ctrl", "I"],
        "format.text.italic",
        ShortcutScope::Text,
    )]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    // No text edit: the shortcut does not fire.
    let no_focus = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::I, ctrl())],
        0.0,
        false,
    );
    assert!(no_focus.intents.is_empty());

    // Frame 1 focuses a text edit; frame 2 dispatches before the
    // widgets, as the shell does.
    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.1,
        true,
    );
    let focused = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::I, ctrl())],
        0.2,
        true,
    );
    let intent = focused
        .intent()
        .expect("a Text shortcut fires in a text edit");
    assert_eq!(intent.command.as_str(), "format.text.italic");
}

#[test]
fn a_selection_shortcut_never_fires_in_phase_4() {
    let mut map = map_of(vec![shortcut(
        &["Ctrl", "Shift", "U"],
        "arrange.ungroup",
        ShortcutScope::Selection,
    )]);
    let app = AppCtx::new(Settings::default());
    assert!(app.selection.is_empty());
    let ctx = Context::default();

    for focus in [
        ShortcutScope::Global,
        ShortcutScope::Page,
        ShortcutScope::CommandBar,
    ] {
        let outcome = frame(
            &ctx,
            &mut map,
            &app,
            focus,
            vec![key_event(Key::U, ctrl_shift())],
            0.0,
            false,
        );
        assert!(outcome.intents.is_empty(), "{focus:?} must not fire");
    }
}

#[test]
fn a_multi_chord_sequence_resolves_after_its_chords() {
    let mut map = map_of(vec![multi_shortcut(
        &[&["Alt", "H"], &["F"]],
        "arrange.group",
        ShortcutScope::Global,
    )]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    let first = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::H, alt())],
        0.0,
        false,
    );
    assert!(first.intents.is_empty());
    assert!(first.pending, "the half-typed sequence is pending");
    assert_eq!(first.events_left, 0, "the chord is consumed");

    let second = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::F, Modifiers::default())],
        0.1,
        false,
    );
    let intent = second.intent().expect("the sequence resolves");
    assert_eq!(intent.command.as_str(), "arrange.group");
    assert!(!second.pending);
}

#[test]
fn a_half_typed_sequence_sets_pending_and_times_out() {
    let mut map = map_of(vec![multi_shortcut(
        &[&["Alt", "H"], &["F"]],
        "arrange.group",
        ShortcutScope::Global,
    )]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    let first = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::H, alt())],
        10.0,
        false,
    );
    assert!(first.pending);

    // Within the timeout the buffer stays.
    let soon = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        11.0,
        false,
    );
    assert!(soon.pending);

    // After 2 seconds (ADR 0036) it is gone.
    let late = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        10.0 + SEQUENCE_TIMEOUT_SECONDS + 0.5,
        false,
    );
    assert!(!late.pending);
    assert!(late.intents.is_empty());
}

#[test]
fn one_real_conflict_warns_once_and_an_exact_repeat_is_silent() {
    let mut map = map_of(vec![
        shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
        shortcut(&["Ctrl", "G"], "arrange.ungroup", ShortcutScope::Global),
        shortcut(
            &["Ctrl", "H"],
            "arrange.flip_horizontal",
            ShortcutScope::Selection,
        ),
        shortcut(
            &["Ctrl", "H"],
            "arrange.flip_horizontal",
            ShortcutScope::Selection,
        ),
    ]);
    // The (Ctrl+G, global) pair names two commands: one warning. The
    // repeated (Ctrl+H, selection) pair names the same command: none.
    assert_eq!(map.len(), 2);
    assert_eq!(map.conflicts().len(), 1);
    assert!(map.conflicts()[0].contains("arrange.group"));
    assert!(map.conflicts()[0].contains("arrange.ungroup"));

    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();
    let outcome = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::G, ctrl())],
        0.0,
        false,
    );
    assert_eq!(
        outcome
            .intent()
            .expect("the first entry wins")
            .command
            .as_str(),
        "arrange.group"
    );
}

#[test]
fn the_most_specific_active_scope_wins() {
    let mut map = map_of(vec![
        shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
        shortcut(&["Ctrl", "G"], "arrange.ungroup", ShortcutScope::Page),
        shortcut(&["Ctrl", "G"], "edit.copy", ShortcutScope::Text),
    ]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    // No page, no text edit: the global binding.
    let global = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::G, ctrl())],
        0.0,
        false,
    );
    assert_eq!(
        global.intent().expect("global binding").command.as_str(),
        "arrange.group"
    );

    // The canvas focused: the page binding.
    let page = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Page,
        vec![key_event(Key::G, ctrl())],
        0.1,
        false,
    );
    assert_eq!(
        page.intent().expect("page binding").command.as_str(),
        "arrange.ungroup"
    );

    // A text edit focused: the text binding, the most specific.
    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.2,
        true,
    );
    let text = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::G, ctrl())],
        0.3,
        true,
    );
    assert_eq!(
        text.intent().expect("text binding").command.as_str(),
        "edit.copy"
    );
}

#[test]
fn ctrl_c_is_edit_copy_on_the_canvas_and_egui_copy_in_a_text_edit() {
    // `Ctrl+C` is `edit.copy` in `global` and `window` (the committed
    // table): it resolves to its command with the canvas focused.
    // Inside a text edit it is egui's own copy (ADR 0036), so the
    // chord is left for the widget.
    let mut map = map_of(committed_shortcuts().entries.clone());
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    let page = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Page,
        vec![key_event(Key::C, ctrl())],
        0.0,
        false,
    );
    assert_eq!(
        page.intent().expect("Ctrl+C on the page").command.as_str(),
        "edit.copy"
    );

    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.1,
        true,
    );
    let text = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::C, ctrl())],
        0.2,
        true,
    );
    assert!(text.intents.is_empty(), "egui copies the text");
    assert_eq!(text.events_left, 1, "the chord stays with egui");
}

#[test]
fn ctrl_shift_c_is_the_format_painter_outside_text_and_align_center_inside() {
    let mut map = map_of(committed_shortcuts().entries.clone());
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    // No text edit: the `global` binding fires.
    let global = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::C, ctrl_shift())],
        0.0,
        false,
    );
    assert_eq!(
        global
            .intent()
            .expect("the format painter")
            .command
            .as_str(),
        "tools.format_painter"
    );

    // Inside a text edit the `text` binding is more specific.
    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.1,
        true,
    );
    let text = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::C, ctrl_shift())],
        0.2,
        true,
    );
    assert_eq!(
        text.intent().expect("the text binding").command.as_str(),
        "format.text.align_center"
    );
}

#[test]
fn typing_and_caret_chords_reach_the_text_edit_untouched() {
    // Chords without Ctrl or Alt are egui's inside a text edit, even
    // when the map binds them (P4-14): the arrows move the caret and
    // Delete deletes text instead of running a command.
    let mut map = map_of(vec![
        shortcut(&["Arrow Keys"], "arrange.nudge", ShortcutScope::Page),
        shortcut(&["Delete"], "edit.clear", ShortcutScope::Window),
    ]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.0,
        true,
    );
    for (key, modifiers) in [
        (Key::ArrowLeft, Modifiers::default()),
        (Key::Delete, Modifiers::default()),
        (Key::F, Modifiers::default()),
    ] {
        let typed = frame(
            &ctx,
            &mut map,
            &app,
            ShortcutScope::Global,
            vec![key_event(key, modifiers)],
            0.1,
            true,
        );
        assert!(typed.intents.is_empty());
        assert_eq!(typed.events_left, 1, "the chord stays with egui");
    }
}

#[test]
fn egui_undo_wins_inside_a_text_edit_and_other_bound_ctrl_chords_dispatch() {
    // Ctrl+Z is `edit.undo` (global), but inside a text edit it is
    // egui's text undo. A bound Ctrl chord egui does not use (Ctrl+G)
    // still dispatches there.
    let mut map = map_of(vec![
        shortcut(&["Ctrl", "Z"], "edit.undo", ShortcutScope::Global),
        shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
    ]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();

    frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        Vec::new(),
        0.0,
        true,
    );
    let undo = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::Z, ctrl())],
        0.1,
        true,
    );
    assert!(undo.intents.is_empty(), "egui undoes the text edit");
    assert_eq!(undo.events_left, 1, "the chord stays with egui");
    let group = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::G, ctrl())],
        0.2,
        true,
    );
    assert_eq!(
        group.intent().expect("Ctrl+G fires").command.as_str(),
        "arrange.group"
    );
    assert_eq!(group.events_left, 0, "the chord is consumed");
}

#[test]
fn an_unmatched_chord_is_left_for_the_widgets() {
    let mut map = map_of(vec![shortcut(
        &["Ctrl", "G"],
        "arrange.group",
        ShortcutScope::Global,
    )]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();
    let outcome = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Global,
        vec![key_event(Key::F5, Modifiers::default())],
        0.0,
        false,
    );
    assert!(outcome.intents.is_empty());
    assert_eq!(outcome.events_left, 1);
}

#[test]
fn the_arrow_keys_dispatch_the_nudge_shortcut_on_a_page() {
    let mut map = map_of(vec![shortcut(
        &["Arrow Keys"],
        "arrange.nudge",
        ShortcutScope::Page,
    )]);
    let app = AppCtx::new(Settings::default());
    let ctx = Context::default();
    let outcome = frame(
        &ctx,
        &mut map,
        &app,
        ShortcutScope::Page,
        vec![key_event(Key::ArrowLeft, Modifiers::default())],
        0.0,
        false,
    );
    assert_eq!(
        outcome.intent().expect("arrows nudge").command.as_str(),
        "arrange.nudge"
    );
}
