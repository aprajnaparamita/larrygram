//! Keyboard shortcut dispatch (ADR 0036): chord sequences from
//! `ui-spec/shortcuts.yaml`, scoped by focus, dispatched as [`Intent`]s.
//!
//! Owned by task P4-08; the conflict rule and the per-scope bindings
//! were amended for task P4-14 (ADR 0036 "Conflicts", amended at the
//! P4-08 merge): the conflict key is the chord sequence **and** the
//! scope, so one sequence may be bound in several scopes and each
//! binding fires when its scope is active. When more than one bound
//! scope is active, the most specific wins: `Text`, then `Selection`,
//! `Page`, `CommandBar`, `Window`, `Global`. Two entries with the same
//! sequence and scope: the first in file order wins and one warning is
//! recorded, except when both name the same command (an exact repeat
//! is dropped silently).
//!
//! A **chord** is one key plus its modifier tokens. Both sides are
//! normalized before matching: modifiers are deduplicated and sorted
//! `Ctrl`, `Shift`, `Alt`, and a shifted symbol key (`+`, `<`, `>`)
//! absorbs the `Shift` modifier (on most layouts those symbols are
//! typed with Shift, and `shortcuts.yaml` lists them inconsistently:
//! `Ctrl+Shift+<` but `Ctrl++`).
//!
//! Multi-chord sequences are stateful: [`ShortcutMap`] buffers the
//! chords typed so far and drops the buffer on a mismatch or after
//! [`SEQUENCE_TIMEOUT_SECONDS`] (the timeout is applied by
//! [`dispatch`], which runs once per frame with the egui frame time;
//! [`ShortcutMap::resolve`] alone is timeless).
//!
//! Focus rules ([`dispatch`]): `Global` and `Window` always match,
//! `Page` and `CommandBar` match when the shell reports that surface as
//! focused, `Text` matches only while an egui text edit has focus, and
//! `Selection` matches only with a non-empty selection (never in
//! Phase 4). While a text edit has focus, the chords without `Ctrl` or
//! `Alt` (typing, shift-typing, the caret keys) are never matched or
//! consumed: they pass straight through to egui. A chord with `Ctrl`
//! or `Alt` is matched even inside a text edit, and the most specific
//! active scope decides what it dispatches (P4-14: `Ctrl+C` resolves
//! in a text edit and `Ctrl+Shift+C` picks the `text` binding there).

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{Context, Event, Key, Modifiers};
use larrygram_commands::{AppCtx, CommandId, Invocation, Source};
use larrygram_ui_spec::{LoadError, ShortcutScope, Shortcuts};

use crate::{Intent, UiError};

/// A multi-chord sequence expires this many seconds after the last
/// typed chord (ADR 0036).
pub const SEQUENCE_TIMEOUT_SECONDS: f64 = 2.0;

/// Pseudo-token for the arrow keys: `shortcuts.yaml` records the
/// held-key note "Arrow Keys" (P1-04), not a direction, so one token
/// covers all four arrow keys.
const ARROWS_TOKEN: &str = "Arrow Keys";

/// Modifier tokens, in canonical order.
const MODIFIER_TOKENS: [&str; 3] = ["Ctrl", "Shift", "Alt"];

/// Key tokens that produce their character with Shift on most
/// layouts. A chord whose key is one of these drops its `Shift`
/// modifier on both sides of the match.
const SHIFTED_SYMBOL_TOKENS: [&str; 3] = ["+", "<", ">"];

/// One chord: modifiers plus one key, as `shortcuts.yaml` token strings
/// (`Ctrl`, `Shift`, `Alt`, `A`, `F1`, ...; ADR 0036).
///
/// The fields are normalized: `modifiers` holds no duplicates and is
/// sorted `Ctrl`, `Shift`, `Alt`, and a shifted symbol key (`+`, `<`,
/// `>`) carries no `Shift` modifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyChord {
    /// The key token (the only non-modifier token of the chord).
    pub key: String,
    /// The modifier tokens, in canonical order (`Ctrl`, `Shift`, `Alt`).
    pub modifiers: Vec<String>,
}

impl KeyChord {
    /// A chord from its key and modifiers, normalized.
    pub fn new(key: impl Into<String>, modifiers: Vec<String>) -> Self {
        normalize_chord(key.into(), modifiers)
    }
}

/// A shortcut entry: what a completed sequence dispatches and where it
/// applies (ADR 0036).
#[derive(Debug, Clone)]
struct Entry {
    /// The command to dispatch.
    command: CommandId,
    /// The scope the shortcut applies in.
    scope: ShortcutScope,
}

impl Entry {
    /// The intent for this entry (ADR 0028 `Source::Shortcut`).
    fn into_intent(self) -> Intent {
        Intent::new(self.command, Invocation::empty(Source::Shortcut))
    }
}

/// One node of the chord-sequence trie.
#[derive(Debug, Default)]
struct Node {
    /// The bindings whose sequence ends here, one per scope (P4-14).
    ends: Vec<Entry>,
    /// Longer sequences continuing from here.
    children: BTreeMap<KeyChord, Node>,
}

/// The shortcut map: typed chords to command intents (ADR 0036).
///
/// Multi-chord sequences are stateful: [`ShortcutMap::resolve`] records
/// the chords typed so far and resets on a mismatch or a timeout.
#[derive(Debug, Default)]
pub struct ShortcutMap {
    /// The chord-sequence trie.
    root: Node,
    /// Number of bindings kept (see [`ShortcutMap::len`]).
    bindings: usize,
    /// The chords typed so far of a half-typed sequence.
    pending: Vec<KeyChord>,
    /// The bindings held back because their sequence is a prefix of
    /// longer ones: the trie waits for the next chord (ADR 0036).
    deferred: Vec<Entry>,
    /// The egui frame time of the last typed chord, for the timeout.
    last_chord_time: Option<f64>,
    /// One line per (sequence, scope) conflict dropped at load (first
    /// wins). An exact repeat is dropped without a line.
    conflicts: Vec<String>,
}

/// The shortcut scopes active at one dispatch (ADR 0036).
///
/// A binding fires when its scope is active: `Global` and `Window`
/// always, `Page` and `CommandBar` when the shell reports that surface as
/// focused, `Text` while an egui text edit has focus, `Selection` with
/// a non-empty selection. When several bindings of one sequence are
/// active, the most specific wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActiveScopes {
    /// A drawing page (the canvas) has focus.
    pub page: bool,
    /// The command_bar has focus.
    pub command_bar: bool,
    /// An egui text edit has focus.
    pub text: bool,
    /// Shapes are selected (Phase 5; false in Phase 4).
    pub selection: bool,
}

impl ActiveScopes {
    /// The scopes active for one dispatch: the focused surface, an
    /// egui text edit and a selection (ADR 0036).
    pub fn new(focus: ShortcutScope, text_edit: bool, selection: bool) -> Self {
        Self {
            page: focus == ShortcutScope::Page,
            command_bar: focus == ShortcutScope::CommandBar,
            text: text_edit,
            selection,
        }
    }

    /// Whether a binding of scope `scope` fires (ADR 0036).
    pub fn contains(&self, scope: ShortcutScope) -> bool {
        match scope {
            ShortcutScope::Global | ShortcutScope::Window => true,
            ShortcutScope::Page => self.page,
            ShortcutScope::CommandBar => self.command_bar,
            ShortcutScope::Text => self.text,
            ShortcutScope::Selection => self.selection,
        }
    }
}

impl ShortcutMap {
    /// Builds the map from `ui-spec/shortcuts.yaml` (ADR 0007).
    ///
    /// Entries are inserted in file order, one binding per (chord
    /// sequence, scope) pair: the same sequence may be bound in
    /// several scopes and each binding is kept (P4-14). A repeated
    /// (sequence, scope) pair is dropped first-wins — silently when it
    /// names the same command (an exact repeat), with one line in
    /// [`ShortcutMap::conflicts`] when it names a different command (a
    /// conflict, ADR 0036 amended). An unknown key token, a chord with
    /// no key or with more than one key, and a shortcut with no chord
    /// are load-time errors.
    pub fn from_shortcuts(shortcuts: &Shortcuts) -> Result<ShortcutMap, UiError> {
        let mut map = ShortcutMap::default();
        for shortcut in &shortcuts.entries {
            if shortcut.sequence.is_empty() {
                return Err(load_error("a shortcut needs at least one chord"));
            }
            let mut chords = Vec::with_capacity(shortcut.sequence.len());
            for tokens in &shortcut.sequence {
                chords.push(chord_from_tokens(tokens)?);
            }
            let command = CommandId::intern(shortcut.command.as_str())
                .map_err(|error| load_error(format!("bad command id: {error}")))?;
            let entry = Entry {
                command,
                scope: shortcut.scope,
            };
            let mut node = &mut map.root;
            for chord in &chords {
                node = node.children.entry(chord.clone()).or_default();
            }
            if let Some(kept) = node.ends.iter().find(|kept| kept.scope == entry.scope) {
                if kept.command != entry.command {
                    map.conflicts.push(format!(
                        "conflicting shortcut [{}] (scope {}): {} wins over {}",
                        sequence_display(&chords),
                        kept.scope.as_str(),
                        kept.command,
                        entry.command,
                    ));
                }
                // An exact repeat (same command) is dropped silently.
                continue;
            }
            node.ends.push(entry);
            map.bindings += 1;
        }
        Ok(map)
    }

    /// Matches typed chords against the map; one [`Intent`] per
    /// completed sequence whose binding fires (ADR 0036): the binding
    /// of the most specific active scope.
    ///
    /// The chords are processed in order and the first completed
    /// sequence wins; the remaining chords are dropped. Each call
    /// extends the half-typed sequence recorded by earlier calls. One
    /// chord can complete two sequences: the held-back completion of a
    /// half-typed sequence fires first and the chord is then tried as
    /// the start of a new sequence (P4-16, the P4-10 finding 2), so
    /// this returns both intents in order. (The ADR 0036 sketch shows
    /// `Option<Intent>`; the signature grew with that rule.) A
    /// completed sequence whose bindings are all inactive dispatches
    /// nothing (the sequence is still consumed).
    pub fn resolve(
        &mut self,
        chords: &[KeyChord],
        active: &ActiveScopes,
    ) -> Result<Vec<Intent>, UiError> {
        Ok(self
            .resolve_entries(chords)
            .iter()
            .filter_map(|entries| pick_entry(entries, active))
            .map(Entry::into_intent)
            .collect())
    }

    /// The conflict warnings recorded at load (ADR 0036 amended): one
    /// line per (sequence, scope) pair dropped in favour of the first
    /// entry when the two name different commands. An exact repeat
    /// (same sequence, scope and command) is dropped without a line.
    pub fn conflicts(&self) -> &[String] {
        &self.conflicts
    }

    /// Whether a multi-chord sequence is half-typed (status bar hint).
    ///
    /// The 2 second timeout is applied by [`dispatch`], which runs
    /// every frame; a stale buffer is reported here until then.
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The chords typed so far of a half-typed sequence.
    pub fn pending_chords(&self) -> &[KeyChord] {
        &self.pending
    }

    /// The half-typed chords as `Alt+H` (the shell's status line).
    pub fn pending_display(&self) -> String {
        sequence_display(&self.pending)
    }

    /// Number of shortcut **bindings** in the map: one per distinct
    /// (chord sequence, scope) pair kept. The same sequence bound in
    /// several scopes counts once per scope. A repeated pair is
    /// dropped first-wins and not counted (an exact repeat silently, a
    /// conflict with one [`ShortcutMap::conflicts`] line).
    pub fn len(&self) -> usize {
        self.bindings
    }

    /// True when the map holds no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings == 0
    }

    /// Feeds the chords to the trie in order; the first chord that
    /// completes a sequence wins and later chords are dropped. One
    /// chord can complete two sequences (the held-back completion of a
    /// half-typed sequence, then its own fresh start); both are
    /// returned, in order.
    fn resolve_entries(&mut self, chords: &[KeyChord]) -> Vec<Vec<Entry>> {
        for chord in chords {
            let chord = normalize_chord(chord.key.clone(), chord.modifiers.clone());
            let fired = self.step(chord, None);
            if !fired.is_empty() {
                return fired;
            }
        }
        Vec::new()
    }

    /// Feeds one typed chord to the trie and returns the bindings it
    /// completed, in order (one group per completed sequence). `now`
    /// is the egui frame time of the chord (when called from
    /// [`dispatch`]).
    ///
    /// A chord that mismatches a half-typed sequence fires the
    /// held-back shorter completion and is then tried as the start of
    /// a fresh sequence (P4-16): it is never dropped without being
    /// tried.
    fn step(&mut self, chord: KeyChord, now: Option<f64>) -> Vec<Vec<Entry>> {
        if let Some(now) = now {
            self.last_chord_time = Some(now);
        }
        let mut candidate = self.pending.clone();
        candidate.push(chord.clone());
        match self.probe(&candidate) {
            // Complete, but longer sequences share the prefix: wait.
            Some((true, Some(entries))) => {
                self.pending = candidate;
                self.deferred = entries;
                Vec::new()
            }
            // Complete and no longer sequence to wait for: fire. Any
            // held-back shorter completion is superseded (`reset`
            // drops it).
            Some((_, Some(entries))) => {
                self.reset();
                vec![entries]
            }
            // A prefix of longer sequences: keep typing.
            Some((true, None)) => {
                self.pending = candidate;
                Vec::new()
            }
            // Mismatch: fire the held-back completion (when one is
            // waiting) and retry the chord as the start of a fresh
            // sequence.
            _ => {
                let mut fired = Vec::new();
                if !self.deferred.is_empty() {
                    fired.push(std::mem::take(&mut self.deferred));
                }
                let had_progress = !self.pending.is_empty();
                self.reset();
                if had_progress || !fired.is_empty() {
                    fired.extend(self.step(chord, now));
                }
                fired
            }
        }
    }

    /// Drops a half-typed sequence after its timeout and returns the
    /// completion that was waiting on a longer sequence (ADR 0036: a
    /// sequence that is a prefix of another fires when the longer one
    /// times out).
    fn expire(&mut self, now: f64) -> Vec<Entry> {
        if self.pending.is_empty() && self.deferred.is_empty() {
            self.last_chord_time = None;
            return Vec::new();
        }
        let expired = self
            .last_chord_time
            .is_some_and(|last| now - last > SEQUENCE_TIMEOUT_SECONDS);
        if expired {
            let fired = std::mem::take(&mut self.deferred);
            self.reset();
            fired
        } else {
            Vec::new()
        }
    }

    /// The trie node at `chords`: `(has_children, bindings)`, or
    /// `None` when the chords are not a path in the trie.
    fn probe(&self, chords: &[KeyChord]) -> Option<(bool, Option<Vec<Entry>>)> {
        let mut node = &self.root;
        for chord in chords {
            node = node.children.get(chord)?;
        }
        let ends = if node.ends.is_empty() {
            None
        } else {
            Some(node.ends.clone())
        };
        Some((!node.children.is_empty(), ends))
    }

    /// Clears the typed-chord buffer, its held-back completion and its
    /// clock. The callers that fire the held-back completion take it
    /// first (a completion superseded by a longer sequence that
    /// completed is dropped here).
    fn reset(&mut self) {
        self.pending.clear();
        self.deferred.clear();
        self.last_chord_time = None;
    }
}

/// Reads egui input, resolves it through `map` and returns the intents
/// of the completed sequences for the given scope (ADR 0036).
///
/// `scope` is the surface the shell reports as focused (`Global` when
/// none of `Page` or `CommandBar` has focus). The bindings of the most
/// specific active scope fire ([`ActiveScopes`], ADR 0036 order). A
/// matched chord is consumed before the widgets see it, even when no
/// binding of its sequence is active; a chord that matches nothing is
/// left alone. While an egui text edit has focus, the chords without
/// `Ctrl` or `Alt` (typing and the caret keys) are never matched or
/// consumed; a chord with `Ctrl` or `Alt` is matched even there. One
/// chord can complete two sequences (the held-back completion of a
/// half-typed sequence, then its own fresh start, P4-16): one intent
/// per fired binding, in order.
pub fn dispatch(
    egui_ctx: &Context,
    map: &mut ShortcutMap,
    app: &AppCtx,
    scope: ShortcutScope,
) -> Result<Vec<Intent>, UiError> {
    let now = egui_ctx.input(|input| input.time);
    let active = ActiveScopes::new(
        scope,
        egui_ctx.text_edit_focused(),
        !app.selection.is_empty(),
    );
    // A sequence that is only a prefix of longer ones fires when the
    // longer ones time out (no key event on that frame).
    let mut fired: Vec<Entry> = pick_entry(&map.expire(now), &active).into_iter().collect();
    if fired.is_empty() {
        let mut consumed: Vec<usize> = Vec::new();

        egui_ctx.input_mut(|input| {
            for (index, event) in input.events.iter().enumerate() {
                if !fired.is_empty() {
                    break;
                }
                let Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    continue;
                };
                let Some(chord) = chord_from_event(*key, *modifiers) else {
                    continue;
                };
                if active.text && defers_to_egui(&chord) {
                    continue;
                }
                let matched = map.step(chord, Some(now));
                if !matched.is_empty() || map.pending() {
                    consumed.push(index);
                }
                for entries in &matched {
                    if let Some(entry) = pick_entry(entries, &active) {
                        fired.push(entry);
                    }
                }
            }
            if !consumed.is_empty() {
                let mut index = 0usize;
                input.events.retain(|_event| {
                    let keep = !consumed.contains(&index);
                    index += 1;
                    keep
                });
            }
        });
    }

    Ok(fired.into_iter().map(Entry::into_intent).collect())
}

/// The binding that fires for a completed sequence (ADR 0036): the
/// one of the most specific active scope.
fn pick_entry(entries: &[Entry], active: &ActiveScopes) -> Option<Entry> {
    entries
        .iter()
        .filter(|entry| active.contains(entry.scope))
        .min_by_key(|entry| scope_rank(entry.scope))
        .cloned()
}

/// Scope specificity, lower wins: `Text`, `Selection`, `Page`,
/// `CommandBar`, `Window`, `Global` (ADR 0036).
fn scope_rank(scope: ShortcutScope) -> u8 {
    match scope {
        ShortcutScope::Text => 0,
        ShortcutScope::Selection => 1,
        ShortcutScope::Page => 2,
        ShortcutScope::CommandBar => 3,
        ShortcutScope::Window => 4,
        ShortcutScope::Global => 5,
    }
}

/// Key tokens egui's text edit binds with `Ctrl` (Cmd on macOS):
/// select all, the clipboard, undo/redo and delete word.
const EGUI_CTRL_TEXT_TOKENS: [&str; 8] = ["A", "C", "V", "X", "Z", "Y", "Backspace", "Delete"];

/// Key tokens egui's text edit binds with `Ctrl+Shift`: redo and
/// select by word.
const EGUI_CTRL_SHIFT_TEXT_TOKENS: [&str; 2] = ["Z", ARROWS_TOKEN];

/// Whether egui's text edit owns this chord while it has focus (ADR
/// 0036 "egui's own shortcuts apply only inside text edits"): plain
/// typing, shift-typing and the caret keys, plus egui's own `Ctrl`
/// editing chords (clipboard, select all, undo/redo, delete word).
/// Every other `Ctrl` or `Alt` chord is ours, so `Ctrl+Shift+C` picks
/// the `text` binding inside a text edit.
fn defers_to_egui(chord: &KeyChord) -> bool {
    let has = |name: &str| chord.modifiers.iter().any(|modifier| modifier == name);
    let key = chord.key.as_str();
    match (has("Ctrl"), has("Shift"), has("Alt")) {
        (_, _, true) => false,
        (false, _, false) => true,
        (true, false, false) => EGUI_CTRL_TEXT_TOKENS.contains(&key),
        (true, true, false) => EGUI_CTRL_SHIFT_TEXT_TOKENS.contains(&key),
    }
}

/// True when `token` is a key or modifier token the egui mapping table
/// covers (the token-coverage check of ADR 0036).
pub fn token_is_known(token: &str) -> bool {
    MODIFIER_TOKENS.contains(&token) || token == ARROWS_TOKEN || key_for_token(token).is_some()
}

/// The `shortcuts.yaml` key token of an egui key press, or `None` for
/// keys the table does not cover (their chords never match).
///
/// The token of the shifted symbol keys depends on Shift being held:
/// `Comma` is `,` and (with Shift) `<`; `Period` is `.` and `>`;
/// `Equals` is `=` and (with Shift, on most layouts) `+`.
///
/// The delete key also depends on the platform (task P7-22): the
/// macOS delete key is `Backspace` to egui (winit `NamedKey::Backspace`),
/// so an unmodified `Backspace` is the `Delete` token — the delete key
/// of either platform matches a `Delete` binding. With a modifier it
/// stays `Backspace` (`Ctrl+Backspace` is delete-back-word and egui's
/// text edit owns it), so a plain `Backspace` chord is never typed and
/// the table has no reachable `Backspace`-only binding.
fn key_token(key: Key, modifiers: Modifiers) -> Option<&'static str> {
    let token = match key {
        Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp => ARROWS_TOKEN,
        Key::Escape => "Esc",
        Key::Tab => "Tab",
        Key::Backspace if modifiers.is_none() => "Delete",
        Key::Backspace => "Backspace",
        Key::Enter => "Enter",
        Key::Space => "Space",
        Key::Insert => "Insert",
        Key::Delete => "Delete",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F1 => "F1",
        Key::F2 => "F2",
        Key::F3 => "F3",
        Key::F4 => "F4",
        Key::F5 => "F5",
        Key::F6 => "F6",
        Key::F7 => "F7",
        Key::F8 => "F8",
        Key::F9 => "F9",
        Key::F10 => "F10",
        Key::F11 => "F11",
        Key::F12 => "F12",
        Key::Comma => {
            if modifiers.shift {
                "<"
            } else {
                ","
            }
        }
        Key::Period => {
            if modifiers.shift {
                ">"
            } else {
                "."
            }
        }
        Key::Equals => {
            if modifiers.shift {
                "+"
            } else {
                "="
            }
        }
        Key::Plus => "+",
        Key::Minus => "-",
        Key::Colon => ":",
        Key::Backslash => "\\",
        Key::Slash => "/",
        Key::Pipe => "|",
        Key::Questionmark => "?",
        Key::Exclamationmark => "!",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::OpenCurlyBracket => "{",
        Key::CloseCurlyBracket => "}",
        Key::Backtick => "`",
        Key::Semicolon => ";",
        Key::Quote => "'",
        _ => return None,
    };
    Some(token)
}

/// The egui key a key token maps to, or `None` for tokens the table
/// does not cover (a load-time error in
/// [`ShortcutMap::from_shortcuts`]).
///
/// `ARROWS_TOKEN` is not a single key; [`token_is_known`] answers for
/// it. `<` and `>` are typed on the comma and period keys (with
/// Shift), so they map there.
fn key_for_token(token: &str) -> Option<Key> {
    let key = match token {
        "Esc" => Key::Escape,
        "Tab" => Key::Tab,
        "Backspace" => Key::Backspace,
        "Enter" => Key::Enter,
        "Space" => Key::Space,
        "Insert" => Key::Insert,
        "Delete" => Key::Delete,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "A" => Key::A,
        "B" => Key::B,
        "C" => Key::C,
        "D" => Key::D,
        "E" => Key::E,
        "F" => Key::F,
        "G" => Key::G,
        "H" => Key::H,
        "I" => Key::I,
        "J" => Key::J,
        "K" => Key::K,
        "L" => Key::L,
        "M" => Key::M,
        "N" => Key::N,
        "O" => Key::O,
        "P" => Key::P,
        "Q" => Key::Q,
        "R" => Key::R,
        "S" => Key::S,
        "T" => Key::T,
        "U" => Key::U,
        "V" => Key::V,
        "W" => Key::W,
        "X" => Key::X,
        "Y" => Key::Y,
        "Z" => Key::Z,
        "0" => Key::Num0,
        "1" => Key::Num1,
        "2" => Key::Num2,
        "3" => Key::Num3,
        "4" => Key::Num4,
        "5" => Key::Num5,
        "6" => Key::Num6,
        "7" => Key::Num7,
        "8" => Key::Num8,
        "9" => Key::Num9,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        "<" | "," => Key::Comma,
        ">" | "." => Key::Period,
        "+" => Key::Plus,
        "=" => Key::Equals,
        "-" => Key::Minus,
        ":" => Key::Colon,
        "\\" => Key::Backslash,
        "/" => Key::Slash,
        "|" => Key::Pipe,
        "?" => Key::Questionmark,
        "!" => Key::Exclamationmark,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        "{" => Key::OpenCurlyBracket,
        "}" => Key::CloseCurlyBracket,
        "`" => Key::Backtick,
        ";" => Key::Semicolon,
        "'" => Key::Quote,
        _ => return None,
    };
    Some(key)
}

/// Normalizes one egui key press to a chord: the key token plus the
/// `Ctrl`, `Shift`, `Alt` modifier tokens (`Ctrl` covers the macOS
/// command key; `shortcuts.yaml` is a Windows table). `None` for keys
/// the token table does not cover.
fn chord_from_event(key: Key, modifiers: Modifiers) -> Option<KeyChord> {
    let key_token = key_token(key, modifiers)?;
    let mut mods = Vec::new();
    if modifiers.ctrl || modifiers.command {
        mods.push("Ctrl".to_owned());
    }
    if modifiers.shift {
        mods.push("Shift".to_owned());
    }
    if modifiers.alt {
        mods.push("Alt".to_owned());
    }
    Some(normalize_chord(key_token.to_owned(), mods))
}

/// Parses one chord of `shortcuts.yaml` (an unordered set of key
/// tokens) into a normalized [`KeyChord`].
fn chord_from_tokens(tokens: &[String]) -> Result<KeyChord, UiError> {
    let mut keys: Vec<&String> = Vec::new();
    let mut modifiers: Vec<String> = Vec::new();
    for token in tokens {
        if MODIFIER_TOKENS.contains(&token.as_str()) {
            modifiers.push(token.clone());
        } else {
            keys.push(token);
        }
    }
    let mut keys = keys.into_iter();
    let Some(key) = keys.next() else {
        return Err(load_error(format!(
            "chord [{}] has no key token",
            tokens.join(", ")
        )));
    };
    if keys.next().is_some() {
        return Err(load_error(format!(
            "chord [{}] has more than one key token",
            tokens.join(", ")
        )));
    }
    if !token_is_known(key) {
        return Err(load_error(format!("unknown key token {key:?}")));
    }
    Ok(normalize_chord(key.clone(), modifiers))
}

/// Normalizes a chord: deduplicated modifiers in canonical order, and
/// no `Shift` on a shifted symbol key.
fn normalize_chord(key: String, mut modifiers: Vec<String>) -> KeyChord {
    modifiers.sort_by_key(|token| modifier_rank(token));
    modifiers.dedup();
    if SHIFTED_SYMBOL_TOKENS.contains(&key.as_str()) {
        modifiers.retain(|token| token != "Shift");
    }
    KeyChord { key, modifiers }
}

/// The canonical order of the modifier tokens.
fn modifier_rank(token: &str) -> u8 {
    match token {
        "Ctrl" => 0,
        "Shift" => 1,
        "Alt" => 2,
        _ => 3,
    }
}

/// The chords of one sequence as `Ctrl+Shift+F, F` for warnings.
fn sequence_display(chords: &[KeyChord]) -> String {
    chords
        .iter()
        .map(|chord| {
            let mut parts: Vec<&str> = chord.modifiers.iter().map(String::as_str).collect();
            parts.push(chord.key.as_str());
            parts.join("+")
        })
        .collect::<Vec<String>>()
        .join(", ")
}

/// A load-time error for the shortcut table (ADR 0007 data errors are
/// [`LoadError::Parse`]).
fn load_error(message: impl Into<String>) -> UiError {
    UiError::Layout(LoadError::Parse {
        path: PathBuf::from("shortcuts.yaml"),
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use larrygram_commands::Settings;
    use larrygram_ui_spec::CommandId as SpecCommandId;

    use super::*;

    fn shortcut(
        tokens: &[&str],
        command: &str,
        scope: ShortcutScope,
    ) -> larrygram_ui_spec::Shortcut {
        larrygram_ui_spec::Shortcut {
            sequence: vec![tokens.iter().map(|t| (*t).to_owned()).collect()],
            command: SpecCommandId::parse(command).expect("id"),
            scope,
            section: None,
        }
    }

    fn multi_shortcut(
        chords: &[&[&str]],
        command: &str,
        scope: ShortcutScope,
    ) -> larrygram_ui_spec::Shortcut {
        larrygram_ui_spec::Shortcut {
            sequence: chords
                .iter()
                .map(|chord| chord.iter().map(|t| (*t).to_owned()).collect())
                .collect(),
            command: SpecCommandId::parse(command).expect("id"),
            scope,
            section: None,
        }
    }

    fn shortcuts(entries: Vec<larrygram_ui_spec::Shortcut>) -> Shortcuts {
        Shortcuts { entries }
    }

    fn chord(key: &str, modifiers: &[&str]) -> KeyChord {
        KeyChord::new(key, modifiers.iter().map(|m| (*m).to_owned()).collect())
    }

    fn app() -> AppCtx {
        AppCtx::new(Settings::default())
    }

    #[test]
    fn a_chord_holds_key_and_modifiers() {
        let chord = KeyChord::new("Z", vec!["Ctrl".into()]);
        assert_eq!(chord.key, "Z");
        assert_eq!(chord.modifiers, vec!["Ctrl".to_owned()]);
    }

    #[test]
    fn modifiers_are_sorted_and_deduplicated() {
        let chord = KeyChord::new("F", vec!["Shift".into(), "Ctrl".into(), "Ctrl".into()]);
        assert_eq!(chord.modifiers, vec!["Ctrl".to_owned(), "Shift".to_owned()]);
    }

    #[test]
    fn a_shifted_symbol_drops_shift_on_both_sides() {
        let from_file = chord_from_tokens(&["Ctrl".to_owned(), "Shift".to_owned(), "<".to_owned()])
            .expect("chord");
        let typed = chord_from_event(
            Key::Comma,
            Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        )
        .expect("chord");
        assert_eq!(from_file, typed);
        assert_eq!(from_file, chord("<", &["Ctrl"]));
    }

    #[test]
    fn the_plus_key_matches_shift_equals_and_numpad_plus() {
        let from_file = chord_from_tokens(&["Ctrl".to_owned(), "+".to_owned()]).expect("chord");
        let shift_equals = chord_from_event(
            Key::Equals,
            Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        )
        .expect("chord");
        let numpad_plus = chord_from_event(
            Key::Plus,
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        )
        .expect("chord");
        assert_eq!(from_file, shift_equals);
        assert_eq!(from_file, numpad_plus);
    }

    #[test]
    fn the_arrow_keys_share_one_token() {
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ] {
            let chord = chord_from_event(key, Modifiers::default()).expect("chord");
            assert_eq!(chord.key, ARROWS_TOKEN);
        }
        let from_file = chord_from_tokens(&[ARROWS_TOKEN.to_owned()]).expect("chord");
        assert_eq!(from_file.key, ARROWS_TOKEN);
    }

    #[test]
    fn an_unknown_key_token_is_a_load_error() {
        let error = chord_from_tokens(&["Hyper".to_owned()]).expect_err("error");
        assert!(error.to_string().contains("unknown key token"));
    }

    #[test]
    fn a_chord_without_a_key_or_with_two_keys_is_a_load_error() {
        let empty = chord_from_tokens(&["Ctrl".to_owned()]).expect_err("error");
        assert!(empty.to_string().contains("no key token"));
        let two = chord_from_tokens(&["A".to_owned(), "B".to_owned()]).expect_err("error");
        assert!(two.to_string().contains("more than one key token"));
    }

    #[test]
    fn every_token_of_the_table_is_known() {
        for token in MODIFIER_TOKENS {
            assert!(token_is_known(token));
        }
        assert!(token_is_known(ARROWS_TOKEN));
        for token in [
            "A",
            "Z",
            "0",
            "9",
            "F1",
            "F12",
            "Esc",
            "Tab",
            "Backspace",
            "Delete",
            "Enter",
            "Space",
            "Insert",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "+",
            "-",
            "=",
            "<",
            ">",
            ",",
            ".",
            ":",
            ";",
            "'",
            "?",
            "!",
            "[",
            "]",
            "{",
            "}",
            "\\",
            "/",
            "|",
            "`",
        ] {
            assert!(key_for_token(token).is_some(), "{token} missing");
            assert!(token_is_known(token), "{token} unknown");
        }
        assert!(!token_is_known("Hyper"));
        assert!(!token_is_known("F13"));
    }

    #[test]
    fn an_uncovered_key_has_no_token() {
        assert_eq!(key_token(Key::F13, Modifiers::default()), None);
        assert!(chord_from_event(Key::F13, Modifiers::default()).is_none());
    }

    #[test]
    fn every_key_of_the_token_table_maps_to_its_token_and_back() {
        // Table-driven over every arm of the `key_token` and
        // `key_for_token` tables (P4-12 coverage major 3): one row per
        // arm, forward and back. The shifted symbol keys read Shift;
        // the arrow keys share one token with no single key back.
        let none = Modifiers::default();
        let shift = Modifiers {
            shift: true,
            ..Default::default()
        };
        let forward: &[(Key, Modifiers, &str)] = &[
            (Key::ArrowDown, none, ARROWS_TOKEN),
            (Key::ArrowLeft, none, ARROWS_TOKEN),
            (Key::ArrowRight, none, ARROWS_TOKEN),
            (Key::ArrowUp, none, ARROWS_TOKEN),
            (Key::Escape, none, "Esc"),
            (Key::Tab, none, "Tab"),
            // The macOS delete key is an unmodified `Backspace` (task
            // P7-22); with a modifier it stays `Backspace`.
            (Key::Backspace, none, "Delete"),
            (Key::Backspace, shift, "Backspace"),
            (Key::Enter, none, "Enter"),
            (Key::Space, none, "Space"),
            (Key::Insert, none, "Insert"),
            (Key::Delete, none, "Delete"),
            (Key::Home, none, "Home"),
            (Key::End, none, "End"),
            (Key::PageUp, none, "PageUp"),
            (Key::PageDown, none, "PageDown"),
            (Key::A, none, "A"),
            (Key::B, none, "B"),
            (Key::C, none, "C"),
            (Key::D, none, "D"),
            (Key::E, none, "E"),
            (Key::F, none, "F"),
            (Key::G, none, "G"),
            (Key::H, none, "H"),
            (Key::I, none, "I"),
            (Key::J, none, "J"),
            (Key::K, none, "K"),
            (Key::L, none, "L"),
            (Key::M, none, "M"),
            (Key::N, none, "N"),
            (Key::O, none, "O"),
            (Key::P, none, "P"),
            (Key::Q, none, "Q"),
            (Key::R, none, "R"),
            (Key::S, none, "S"),
            (Key::T, none, "T"),
            (Key::U, none, "U"),
            (Key::V, none, "V"),
            (Key::W, none, "W"),
            (Key::X, none, "X"),
            (Key::Y, none, "Y"),
            (Key::Z, none, "Z"),
            (Key::Num0, none, "0"),
            (Key::Num1, none, "1"),
            (Key::Num2, none, "2"),
            (Key::Num3, none, "3"),
            (Key::Num4, none, "4"),
            (Key::Num5, none, "5"),
            (Key::Num6, none, "6"),
            (Key::Num7, none, "7"),
            (Key::Num8, none, "8"),
            (Key::Num9, none, "9"),
            (Key::F1, none, "F1"),
            (Key::F2, none, "F2"),
            (Key::F3, none, "F3"),
            (Key::F4, none, "F4"),
            (Key::F5, none, "F5"),
            (Key::F6, none, "F6"),
            (Key::F7, none, "F7"),
            (Key::F8, none, "F8"),
            (Key::F9, none, "F9"),
            (Key::F10, none, "F10"),
            (Key::F11, none, "F11"),
            (Key::F12, none, "F12"),
            (Key::Comma, none, ","),
            (Key::Comma, shift, "<"),
            (Key::Period, none, "."),
            (Key::Period, shift, ">"),
            (Key::Equals, none, "="),
            (Key::Equals, shift, "+"),
            (Key::Plus, none, "+"),
            (Key::Minus, none, "-"),
            (Key::Colon, none, ":"),
            (Key::Backslash, none, "\\"),
            (Key::Slash, none, "/"),
            (Key::Pipe, none, "|"),
            (Key::Questionmark, none, "?"),
            (Key::Exclamationmark, none, "!"),
            (Key::OpenBracket, none, "["),
            (Key::CloseBracket, none, "]"),
            (Key::OpenCurlyBracket, none, "{"),
            (Key::CloseCurlyBracket, none, "}"),
            (Key::Backtick, none, "`"),
            (Key::Semicolon, none, ";"),
            (Key::Quote, none, "'"),
        ];
        for (key, modifiers, token) in forward {
            assert_eq!(key_token(*key, *modifiers), Some(*token), "{key:?}");
        }
        let backward: &[(&str, Key)] = &[
            ("Esc", Key::Escape),
            ("Tab", Key::Tab),
            ("Backspace", Key::Backspace),
            ("Enter", Key::Enter),
            ("Space", Key::Space),
            ("Insert", Key::Insert),
            ("Delete", Key::Delete),
            ("Home", Key::Home),
            ("End", Key::End),
            ("PageUp", Key::PageUp),
            ("PageDown", Key::PageDown),
            ("A", Key::A),
            ("B", Key::B),
            ("C", Key::C),
            ("D", Key::D),
            ("E", Key::E),
            ("F", Key::F),
            ("G", Key::G),
            ("H", Key::H),
            ("I", Key::I),
            ("J", Key::J),
            ("K", Key::K),
            ("L", Key::L),
            ("M", Key::M),
            ("N", Key::N),
            ("O", Key::O),
            ("P", Key::P),
            ("Q", Key::Q),
            ("R", Key::R),
            ("S", Key::S),
            ("T", Key::T),
            ("U", Key::U),
            ("V", Key::V),
            ("W", Key::W),
            ("X", Key::X),
            ("Y", Key::Y),
            ("Z", Key::Z),
            ("0", Key::Num0),
            ("1", Key::Num1),
            ("2", Key::Num2),
            ("3", Key::Num3),
            ("4", Key::Num4),
            ("5", Key::Num5),
            ("6", Key::Num6),
            ("7", Key::Num7),
            ("8", Key::Num8),
            ("9", Key::Num9),
            ("F1", Key::F1),
            ("F2", Key::F2),
            ("F3", Key::F3),
            ("F4", Key::F4),
            ("F5", Key::F5),
            ("F6", Key::F6),
            ("F7", Key::F7),
            ("F8", Key::F8),
            ("F9", Key::F9),
            ("F10", Key::F10),
            ("F11", Key::F11),
            ("F12", Key::F12),
            (",", Key::Comma),
            ("<", Key::Comma),
            (">", Key::Period),
            (".", Key::Period),
            ("+", Key::Plus),
            ("=", Key::Equals),
            ("-", Key::Minus),
            (":", Key::Colon),
            ("\\", Key::Backslash),
            ("/", Key::Slash),
            ("|", Key::Pipe),
            ("?", Key::Questionmark),
            ("!", Key::Exclamationmark),
            ("[", Key::OpenBracket),
            ("]", Key::CloseBracket),
            ("{", Key::OpenCurlyBracket),
            ("}", Key::CloseCurlyBracket),
            ("`", Key::Backtick),
            (";", Key::Semicolon),
            ("'", Key::Quote),
        ];
        for (token, key) in backward {
            assert_eq!(key_for_token(token), Some(*key), "{token}");
        }
        // The arrows round-trip through their shared token (no single
        // key back), and the uncovered keys have neither.
        assert!(token_is_known(ARROWS_TOKEN));
        assert_eq!(key_for_token(ARROWS_TOKEN), None);
        assert_eq!(key_token(Key::F13, none), None);
        assert_eq!(key_for_token("Hyper"), None);
    }

    #[test]
    fn the_map_loads_and_resolves_single_chords() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "Z"], "edit.undo", ShortcutScope::Global),
            shortcut(&["Ctrl", "I"], "format.text.italic", ShortcutScope::Text),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert_eq!(map.len(), 2);
        assert!(!map.is_empty());
        assert!(map.conflicts().is_empty());
        let intent = map
            .resolve(&[chord("Z", &["Ctrl"])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "edit.undo");
        assert_eq!(intent.invocation.source, Source::Shortcut);
        assert!(!map.pending());
    }

    #[test]
    fn a_leaf_holds_one_binding_per_scope() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Global),
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Window),
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Page),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert_eq!(map.len(), 3, "one binding per (sequence, scope) pair");
        assert!(map.conflicts().is_empty());
        // All three scopes name the same command: it fires anywhere.
        for active in [
            ActiveScopes::default(),
            ActiveScopes::new(ShortcutScope::Page, false, false),
            ActiveScopes::new(ShortcutScope::Global, true, false),
        ] {
            let intent = map
                .resolve(&[chord("C", &["Ctrl"])], &active)
                .expect("resolve")
                .remove(0);
            assert_eq!(intent.command.as_str(), "edit.copy");
        }
    }

    #[test]
    fn the_most_specific_active_scope_wins() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
            shortcut(&["Ctrl", "G"], "arrange.ungroup", ShortcutScope::Page),
            shortcut(&["Ctrl", "G"], "edit.copy", ShortcutScope::Text),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert_eq!(map.len(), 3);
        assert!(map.conflicts().is_empty(), "different scopes are legal");

        // Only Global active: the global binding.
        let global = map
            .resolve(&[chord("G", &["Ctrl"])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(global.command.as_str(), "arrange.group");

        // Page beats Global (ADR 0036 order).
        let page = map
            .resolve(
                &[chord("G", &["Ctrl"])],
                &ActiveScopes::new(ShortcutScope::Page, false, false),
            )
            .expect("resolve")
            .remove(0);
        assert_eq!(page.command.as_str(), "arrange.ungroup");

        // Text beats Page and Global.
        let text = map
            .resolve(
                &[chord("G", &["Ctrl"])],
                &ActiveScopes::new(ShortcutScope::Page, true, false),
            )
            .expect("resolve")
            .remove(0);
        assert_eq!(text.command.as_str(), "edit.copy");
    }

    #[test]
    fn an_exact_repeat_is_dropped_without_a_warning() {
        let map_source = shortcuts(vec![
            shortcut(
                &["Ctrl", "H"],
                "arrange.flip_horizontal",
                ShortcutScope::Global,
            ),
            shortcut(
                &["Ctrl", "H"],
                "arrange.flip_horizontal",
                ShortcutScope::Global,
            ),
            shortcut(
                &["Ctrl", "H"],
                "arrange.flip_horizontal",
                ShortcutScope::Page,
            ),
        ]);
        let map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert_eq!(map.len(), 2, "the exact repeat is dropped");
        assert!(map.conflicts().is_empty(), "an exact repeat is silent");
    }

    #[test]
    fn a_real_conflict_is_recorded_once_and_first_wins() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
            shortcut(&["Ctrl", "G"], "arrange.ungroup", ShortcutScope::Global),
            shortcut(&["Ctrl", "G"], "edit.copy", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert_eq!(map.len(), 1);
        assert_eq!(map.conflicts().len(), 2);
        assert!(map.conflicts()[0].contains("arrange.group"));
        assert!(map.conflicts()[0].contains("arrange.ungroup"));
        let intent = map
            .resolve(&[chord("G", &["Ctrl"])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "arrange.group");
    }

    #[test]
    fn a_completed_sequence_with_no_active_binding_resolves_to_nothing() {
        let map_source = shortcuts(vec![shortcut(
            &["Ctrl", "G"],
            "arrange.group",
            ShortcutScope::Selection,
        )]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert!(
            map.resolve(&[chord("G", &["Ctrl"])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        assert!(!map.pending());
    }

    #[test]
    fn len_counts_bindings_not_source_entries() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Global),
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Window),
            shortcut(&["Ctrl", "C"], "edit.copy", ShortcutScope::Window),
            shortcut(&["Ctrl", "G"], "arrange.group", ShortcutScope::Global),
            shortcut(&["Ctrl", "G"], "arrange.ungroup", ShortcutScope::Global),
        ]);
        let map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        // Three bindings: Ctrl+C global, Ctrl+C window (the repeat is
        // dropped) and Ctrl+G global (the conflict is dropped).
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn pending_chords_show_as_a_status_line() {
        let two_chords = multi_shortcut(
            &[&["Alt", "H"], &["F"]],
            "arrange.group",
            ShortcutScope::Global,
        );
        let mut map = ShortcutMap::from_shortcuts(&shortcuts(vec![two_chords])).expect("map");
        assert_eq!(map.pending_display(), "");
        map.step(chord("H", &["Alt"]), Some(1.0));
        assert!(map.pending());
        assert_eq!(map.pending_chords(), &[chord("H", &["Alt"])]);
        assert_eq!(map.pending_display(), "Alt+H");
    }

    #[test]
    fn a_multi_chord_sequence_resolves_after_its_chords() {
        let two_chords = multi_shortcut(
            &[&["Alt", "H"], &["F"]],
            "arrange.group",
            ShortcutScope::Global,
        );
        let mut map = ShortcutMap::from_shortcuts(&shortcuts(vec![two_chords])).expect("map");
        assert!(
            map.resolve(&[chord("H", &["Alt"])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        assert!(map.pending());
        let intent = map
            .resolve(&[chord("F", &[])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "arrange.group");
        assert!(!map.pending());
    }

    #[test]
    fn a_mismatch_resets_the_buffer_and_retries_the_chord() {
        let two_chords = multi_shortcut(
            &[&["Alt", "H"], &["F"]],
            "arrange.group",
            ShortcutScope::Global,
        );
        let single = shortcut(&["Ctrl", "Z"], "edit.undo", ShortcutScope::Global);
        let mut map =
            ShortcutMap::from_shortcuts(&shortcuts(vec![two_chords, single])).expect("map");
        assert!(
            map.resolve(&[chord("H", &["Alt"])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        // Ctrl+Z does not continue the sequence: it starts a new one.
        let intent = map
            .resolve(&[chord("Z", &["Ctrl"])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "edit.undo");
        assert!(!map.pending());
    }

    #[test]
    fn a_prefix_entry_waits_then_fires_on_mismatch() {
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert!(
            map.resolve(&[chord("F6", &[])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        assert!(map.pending());
        // F5 does not continue the longer sequence: the shorter one fires.
        let intent = map
            .resolve(&[chord("F5", &[])], &ActiveScopes::default())
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "view.zoom.in");
        assert!(!map.pending());
    }

    #[test]
    fn a_deferred_completion_fires_the_most_specific_active_binding() {
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            shortcut(&["F6"], "arrange.nudge", ShortcutScope::Page),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert!(
            map.resolve(&[chord("F6", &[])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        let intent = map
            .resolve(
                &[chord("F5", &[])],
                &ActiveScopes::new(ShortcutScope::Page, false, false),
            )
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "arrange.nudge");
    }

    #[test]
    fn an_expired_sequence_drops_the_buffer_and_fires_the_deferred_entry() {
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        map.step(chord("F6", &[]), Some(10.0));
        assert!(map.pending());
        assert!(map.expire(11.0).is_empty());
        let entries = map.expire(12.5);
        assert_eq!(entries.len(), 1, "the deferred binding fires");
        assert_eq!(entries[0].command.as_str(), "view.zoom.in");
        assert!(!map.pending());
    }

    #[test]
    fn a_half_typed_sequence_times_out() {
        let two_chords = multi_shortcut(
            &[&["Alt", "H"], &["F"]],
            "arrange.group",
            ShortcutScope::Global,
        );
        let mut map = ShortcutMap::from_shortcuts(&shortcuts(vec![two_chords])).expect("map");
        map.step(chord("H", &["Alt"]), Some(10.0));
        assert!(map.pending());
        assert!(map.expire(10.0 + SEQUENCE_TIMEOUT_SECONDS / 2.0).is_empty());
        assert!(map.pending());
        assert!(map.expire(10.0 + SEQUENCE_TIMEOUT_SECONDS + 0.1).is_empty());
        assert!(!map.pending());
    }

    #[test]
    fn an_empty_table_loads_as_an_empty_map() {
        let mut map = ShortcutMap::from_shortcuts(&Shortcuts::default()).expect("map");
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert!(!map.pending());
        assert!(
            map.resolve(&[], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
    }

    #[test]
    fn a_shortcut_without_a_chord_is_a_load_error() {
        let empty = larrygram_ui_spec::Shortcut {
            sequence: Vec::new(),
            command: SpecCommandId::parse("edit.undo").expect("id"),
            scope: ShortcutScope::Global,
            section: None,
        };
        let error = ShortcutMap::from_shortcuts(&shortcuts(vec![empty])).expect_err("error");
        assert!(error.to_string().contains("at least one chord"));
    }

    #[test]
    fn a_bad_token_is_a_load_error() {
        let bad = shortcut(&["Ctrl", "Warp"], "edit.undo", ShortcutScope::Global);
        let error = ShortcutMap::from_shortcuts(&shortcuts(vec![bad])).expect_err("error");
        assert!(error.to_string().contains("unknown key token"));
    }

    #[test]
    fn active_scopes_follow_the_focus() {
        let page = ActiveScopes::new(ShortcutScope::Page, false, false);
        assert!(page.page);
        assert!(!page.command_bar);
        assert!(!page.text);
        assert!(!page.selection);
        let command_bar = ActiveScopes::new(ShortcutScope::CommandBar, true, true);
        assert!(command_bar.command_bar);
        assert!(command_bar.text);
        assert!(command_bar.selection);
        assert!(!command_bar.page);
    }

    #[test]
    fn global_and_window_scopes_always_match() {
        for want in [ShortcutScope::Global, ShortcutScope::Window] {
            for active in [
                ActiveScopes::default(),
                ActiveScopes::new(ShortcutScope::Page, false, false),
                ActiveScopes::new(ShortcutScope::CommandBar, true, false),
                ActiveScopes::new(ShortcutScope::Global, false, true),
            ] {
                assert!(active.contains(want));
            }
        }
    }

    #[test]
    fn page_and_command_bar_need_their_surface() {
        let page = ActiveScopes::new(ShortcutScope::Page, false, false);
        let command_bar = ActiveScopes::new(ShortcutScope::CommandBar, false, false);
        assert!(page.contains(ShortcutScope::Page));
        assert!(!page.contains(ShortcutScope::CommandBar));
        assert!(command_bar.contains(ShortcutScope::CommandBar));
        assert!(!command_bar.contains(ShortcutScope::Page));
    }

    #[test]
    fn text_needs_a_text_edit_and_selection_never_fires_in_phase_4() {
        let with_text = ActiveScopes::new(ShortcutScope::Global, true, false);
        let without_text = ActiveScopes::new(ShortcutScope::Global, false, false);
        assert!(with_text.contains(ShortcutScope::Text));
        assert!(!without_text.contains(ShortcutScope::Text));
        for active in [
            ActiveScopes::default(),
            ActiveScopes::new(ShortcutScope::Page, true, false),
        ] {
            assert!(!active.contains(ShortcutScope::Selection));
        }
    }

    #[test]
    fn plain_typing_and_caret_keys_defer_to_egui() {
        for key in ["A", "1", "-", ARROWS_TOKEN, "Delete", "Esc", "Home"] {
            assert!(defers_to_egui(&chord(key, &[])), "{key}");
        }
        assert!(defers_to_egui(&chord("A", &["Shift"])));
    }

    #[test]
    fn egui_text_editing_chords_defer() {
        for key in ["A", "C", "V", "X", "Z", "Y", "Backspace", "Delete"] {
            assert!(defers_to_egui(&chord(key, &["Ctrl"])), "Ctrl+{key}");
        }
        assert!(defers_to_egui(&chord("Z", &["Ctrl", "Shift"])));
        assert!(defers_to_egui(&chord(ARROWS_TOKEN, &["Ctrl", "Shift"])));
    }

    #[test]
    fn other_ctrl_and_alt_chords_do_not_defer() {
        // Ctrl+Shift+C picks the `text` binding inside a text edit.
        assert!(!defers_to_egui(&chord("C", &["Ctrl", "Shift"])));
        assert!(!defers_to_egui(&chord("I", &["Ctrl"])));
        assert!(!defers_to_egui(&chord("<", &["Ctrl"])));
        assert!(!defers_to_egui(&chord("F4", &["Alt"])));
        assert!(!defers_to_egui(&chord("C", &["Ctrl", "Alt"])));
    }

    #[test]
    fn chord_from_event_maps_modifiers() {
        let typed = chord_from_event(
            Key::F,
            Modifiers {
                ctrl: true,
                shift: true,
                alt: true,
                ..Default::default()
            },
        )
        .expect("chord");
        assert_eq!(typed, chord("F", &["Ctrl", "Shift", "Alt"]));
        // The macOS command key reads as Ctrl (a Windows table).
        let command_key = chord_from_event(
            Key::S,
            Modifiers {
                mac_cmd: true,
                command: true,
                ..Default::default()
            },
        )
        .expect("chord");
        assert_eq!(command_key, chord("S", &["Ctrl"]));
    }

    #[test]
    fn resolve_processes_one_chord_per_call_and_drops_the_rest() {
        let map_source = shortcuts(vec![
            shortcut(&["Ctrl", "Z"], "edit.undo", ShortcutScope::Global),
            shortcut(&["Ctrl", "Y"], "edit.redo", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        let intent = map
            .resolve(
                &[chord("Z", &["Ctrl"]), chord("Y", &["Ctrl"])],
                &ActiveScopes::default(),
            )
            .expect("resolve")
            .remove(0);
        assert_eq!(intent.command.as_str(), "edit.undo");
    }

    #[test]
    fn the_context_dispatch_sees_no_keys() {
        let map_source = shortcuts(vec![shortcut(
            &["Ctrl", "Z"],
            "edit.undo",
            ShortcutScope::Global,
        )]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        let ctx = Context::default();
        let mut outcome = None;
        let mut output = ctx.run_ui(Default::default(), |ctx| {
            outcome = dispatch(ctx, &mut map, &app(), ShortcutScope::Global).ok();
        });
        output.textures_delta.clear();
        assert_eq!(outcome, Some(Vec::new()));
    }

    #[test]
    fn a_mismatch_that_fires_a_held_back_completion_retries_the_chord() {
        // P4-16 Goal 5 (the P4-10 finding 2): the chord that
        // mismatches a half-typed sequence is not dropped when it
        // fires the held-back completion; it is tried as the start of
        // a new sequence and its own binding fires too.
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
            shortcut(&["F5"], "view.zoom.out", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        assert!(
            map.resolve(&[chord("F6", &[])], &ActiveScopes::default())
                .expect("resolve")
                .is_empty()
        );
        assert!(map.pending());
        // F5 does not continue the longer sequence: the held-back
        // `view.zoom.in` fires and F5 starts (and completes) its own
        // sequence.
        let intents = map
            .resolve(&[chord("F5", &[])], &ActiveScopes::default())
            .expect("resolve");
        let ids: Vec<&str> = intents
            .iter()
            .map(|intent| intent.command.as_str())
            .collect();
        assert_eq!(ids, vec!["view.zoom.in", "view.zoom.out"]);
        assert!(!map.pending());
    }

    #[test]
    fn a_mismatching_chord_can_start_a_new_sequence() {
        // The same rule where the chord begins a multi-chord sequence:
        // the held-back completion fires and the chord becomes the
        // start of the new pending sequence.
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
            multi_shortcut(
                &[&["Ctrl", "G"], &["X"]],
                "arrange.group",
                ShortcutScope::Global,
            ),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        map.step(chord("F6", &[]), Some(1.0));
        assert!(map.pending());
        let intents = map
            .resolve(&[chord("G", &["Ctrl"])], &ActiveScopes::default())
            .expect("resolve");
        assert_eq!(intents.len(), 1, "the held-back completion fires");
        assert_eq!(intents[0].command.as_str(), "view.zoom.in");
        assert!(map.pending(), "the chord starts the new sequence");
        assert_eq!(map.pending_display(), "Ctrl+G");
        let intents = map
            .resolve(&[chord("X", &[])], &ActiveScopes::default())
            .expect("resolve");
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].command.as_str(), "arrange.group");
    }

    #[test]
    fn a_completed_longer_sequence_drops_the_held_back_completion() {
        // The held-back completion is superseded when the longer
        // sequence completes: it must not fire on a later mismatch.
        let map_source = shortcuts(vec![
            shortcut(&["F6"], "view.zoom.in", ShortcutScope::Global),
            multi_shortcut(&[&["F6"], &["F6"]], "view.zoom.fit", ShortcutScope::Global),
        ]);
        let mut map = ShortcutMap::from_shortcuts(&map_source).expect("map");
        map.step(chord("F6", &[]), Some(1.0));
        map.step(chord("F6", &[]), Some(1.5));
        assert!(!map.pending());
        let fired = map.step(chord("W", &[]), Some(2.0));
        assert!(fired.is_empty(), "no stale completion: {fired:?}");
    }
}
