//! Item resolution for the Panel Classic widgets (ADR 0031): one
//! [`Row`] per drawn menu or popup entry.
//!
//! The rows are plain data: the menus, the toolbar popups and the
//! context menus all paint the same shape (gutter, label, shortcut,
//! submenu arrow) and all dispatch through [`Activation`]. Nothing
//! here executes a command (ADR 0028): an activation is an [`Intent`],
//! a disabled reason, a submenu or a toolbar-visibility toggle.

use larrygram_commands::{ArgValue, Args, CommandId, CommandStatus, Invocation, Source};
use larrygram_ui_spec::embedded::is_out_of_scope;
use larrygram_ui_spec::v2::{Item, ItemKind};

use crate::Intent;
use crate::panel_classic::{
    MAX_DEPTH, NOT_YET_IMPLEMENTED, OUT_OF_SCOPE, PanelClassic, PanelInput,
};

/// What activating one row does (ADR 0031).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Activation {
    /// Dispatch this intent.
    Intent(Intent),
    /// Render the row disabled with this tooltip (empty: no tooltip).
    Disabled {
        /// The tooltip text (ours, never Microsoft's).
        reason: String,
    },
    /// The row has children (the `▸` rows).
    Submenu,
    /// A non-clickable heading (a palette section title).
    Heading,
    /// A View > Toolbars row: flip the visibility of `toolbar` and
    /// dispatch `intent` when the command has one.
    ToolbarToggle {
        /// The toolbar id (`toolbar.format_text`).
        toolbar: String,
        /// The row's command, when it resolves.
        intent: Option<Intent>,
    },
}

/// One drawn row of a menu or popup.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    /// The placement id of the item (or `"<list>.<n>"` for a
    /// run-time entry).
    pub id: String,
    /// The row label.
    pub label: String,
    /// Access-key letter.
    pub access: Option<char>,
    /// Shortcut text, right-aligned in menus.
    pub shortcut: Option<String>,
    /// Icon key into `ui-spec/icons.yaml`.
    pub icon: Option<String>,
    /// Check state (`Some` draws the mark).
    pub check: Option<bool>,
    /// Tooltip: "Label (Shortcut)", or the disabled reason.
    pub tooltip: String,
    /// What activation does.
    pub activation: Activation,
    /// Submenu rows, built eagerly (bounded by [`MAX_DEPTH`]).
    pub children: Vec<Row>,
    /// Draw a separator line before this row (`begin_group`).
    pub separator: bool,
}

impl Row {
    /// Whether the row takes keyboard focus (not a heading).
    pub fn focusable(&self) -> bool {
        !matches!(self.activation, Activation::Heading)
    }

    /// Whether the row opens a submenu.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Builds the rows of one menu level (ADR 0031 control-to-command
/// rules), skipping `hidden` items and expanding run-time lists.
///
/// `in_toolbars_menu` marks the View > Toolbars submenu: its rows
/// toggle toolbar visibility instead of dispatching alone.
pub(crate) fn rows(
    items: &[Item],
    input: &PanelInput<'_>,
    state: &PanelClassic,
    source: Source,
    in_toolbars_menu: bool,
    depth: u32,
) -> Vec<Row> {
    let mut out = Vec::new();
    for item in items {
        if item.hidden {
            continue;
        }
        let mut separator = item.begin_group;
        match item.kind {
            ItemKind::List => {
                let name = item.list.as_deref().unwrap_or_default();
                let entries = input.lists.list(name, input.app);
                if entries.is_empty() {
                    out.push(Row {
                        id: item.id.clone(),
                        label: "(none)".to_owned(),
                        access: None,
                        shortcut: None,
                        icon: None,
                        check: None,
                        tooltip: String::new(),
                        activation: Activation::Disabled {
                            reason: String::new(),
                        },
                        children: Vec::new(),
                        separator,
                    });
                }
                for (index, entry) in entries.iter().enumerate() {
                    let activation = match &entry.command {
                        Some(command) => disabled_or(command_activation(
                            command.as_str(),
                            input,
                            source,
                            entry.args.clone(),
                        )),
                        None => Activation::Disabled {
                            reason: NOT_YET_IMPLEMENTED.to_owned(),
                        },
                    };
                    let tooltip = tooltip_for(&entry.label, None, &activation);
                    out.push(Row {
                        id: format!("{}.{}", item.id, index),
                        label: entry.label.clone(),
                        access: entry.access,
                        shortcut: None,
                        icon: entry.icon.clone(),
                        check: Some(entry.checked),
                        tooltip,
                        activation,
                        children: Vec::new(),
                        separator,
                    });
                    separator = false;
                }
            }
            ItemKind::Options => {
                for (index, option) in item.options.iter().enumerate() {
                    let mut args = Args::new();
                    args.set("value", ArgValue::Text(option.clone()));
                    let activation = match &item.command {
                        Some(command) => {
                            disabled_or(command_activation(command.as_str(), input, source, args))
                        }
                        None => Activation::Disabled {
                            reason: NOT_YET_IMPLEMENTED.to_owned(),
                        },
                    };
                    let tooltip = tooltip_for(option, None, &activation);
                    out.push(Row {
                        id: format!("{}.{}", item.id, index),
                        label: option.clone(),
                        access: None,
                        shortcut: None,
                        icon: None,
                        check: None,
                        tooltip,
                        activation,
                        children: Vec::new(),
                        separator,
                    });
                    separator = false;
                }
            }
            ItemKind::Heading => {
                out.push(Row {
                    id: item.id.clone(),
                    label: item.label.clone().unwrap_or_default(),
                    access: item.access,
                    shortcut: None,
                    icon: None,
                    check: None,
                    tooltip: String::new(),
                    activation: Activation::Heading,
                    children: Vec::new(),
                    separator,
                });
            }
            _ => {
                let label = item.label.clone().unwrap_or_default();
                let children = if item.items.is_empty() || depth >= MAX_DEPTH {
                    Vec::new()
                } else {
                    rows(
                        &item.items,
                        input,
                        state,
                        source,
                        item.label.as_deref() == Some("Toolbars"),
                        depth + 1,
                    )
                };
                let activation = if in_toolbars_menu {
                    toolbar_toggle(item, input, source, state)
                } else if item.items.is_empty() {
                    disabled_or(item_activation(item, input, source, Args::new()))
                } else {
                    Activation::Submenu
                };
                let tooltip = tooltip_for(&label, item.shortcut.as_deref(), &activation);
                out.push(Row {
                    id: item.id.clone(),
                    label,
                    access: item.access,
                    shortcut: item.shortcut.clone(),
                    icon: item.icon.clone(),
                    check: item_check(item, input, state, in_toolbars_menu),
                    tooltip,
                    activation,
                    children,
                    separator,
                });
            }
        }
    }
    out
}

/// A View > Toolbars row: toggles the toolbar with the same label.
fn toolbar_toggle(
    item: &Item,
    input: &PanelInput<'_>,
    source: Source,
    state: &PanelClassic,
) -> Activation {
    let label = item.label.as_deref().unwrap_or_default();
    let toolbar = state.toolbar_id_for_label(input.layout, label);
    let intent = match &item.command {
        Some(command) => command_activation(command.as_str(), input, source, Args::new())
            .ok()
            .and_then(|ok| match ok {
                Activation::Intent(intent) => Some(intent),
                _ => None,
            }),
        None => None,
    };
    Activation::ToolbarToggle { toolbar, intent }
}

/// The check state of a row: the toolbar visibility for the
/// View > Toolbars rows, else the command's `checked` state (the
/// registry has no checked seam yet; the caller provides one).
pub(crate) fn item_check(
    item: &Item,
    input: &PanelInput<'_>,
    state: &PanelClassic,
    in_toolbars_menu: bool,
) -> Option<bool> {
    if in_toolbars_menu {
        let label = item.label.as_deref().unwrap_or_default();
        let toolbar = state.toolbar_id_for_label(input.layout, label);
        return Some(state.toolbar_visible_in(input.layout, input.mode, &toolbar));
    }
    if !matches!(item.kind, ItemKind::Check | ItemKind::Toggle) {
        return None;
    }
    let command = item.command.as_ref()?;
    let checked = input.checked?;
    checked.is_checked(command.as_str(), input.app)
}

/// The ADR 0031 activation of one plain item (also the toolbar
/// buttons).
pub(crate) fn item_activation(
    item: &Item,
    input: &PanelInput<'_>,
    source: Source,
    args: Args,
) -> Result<Activation, String> {
    if item
        .fluent
        .as_ref()
        .is_some_and(|fluent| is_out_of_scope(fluent.as_str()))
    {
        return Err(OUT_OF_SCOPE.to_owned());
    }
    let Some(command) = &item.command else {
        return Err(NOT_YET_IMPLEMENTED.to_owned());
    };
    command_activation(command.as_str(), input, source, args)
}

/// The activation for a registry id (ADR 0031): an [`Intent`] when the
/// command is implemented and enabled, else the tooltip reason.
pub(crate) fn command_activation(
    command: &str,
    input: &PanelInput<'_>,
    source: Source,
    args: Args,
) -> Result<Activation, String> {
    let id = CommandId::intern(command).map_err(|_| NOT_YET_IMPLEMENTED.to_owned())?;
    if input.registry.status(&id) != CommandStatus::Implemented {
        return Err(NOT_YET_IMPLEMENTED.to_owned());
    }
    let enabled = input.registry.enabled(&id, input.app);
    if !enabled.is_enabled() {
        return Err(enabled
            .reason
            .unwrap_or_else(|| NOT_YET_IMPLEMENTED.to_owned()));
    }
    Ok(Activation::Intent(Intent::new(
        id,
        Invocation::new(args, source),
    )))
}

/// A failed activation renders disabled with its reason.
pub(crate) fn disabled_or(result: Result<Activation, String>) -> Activation {
    result.unwrap_or_else(|reason| Activation::Disabled { reason })
}

/// The activation of one toolbar button (its `command`, ADR 0031).
pub(crate) fn button_activation(
    item: &Item,
    input: &PanelInput<'_>,
    source: Source,
    args: Args,
) -> Activation {
    disabled_or(item_activation(item, input, source, args))
}

/// The activation of a combo with `value` typed in it.
pub(crate) fn combo_activation(
    item: &Item,
    input: &PanelInput<'_>,
    source: Source,
    value: String,
) -> Activation {
    let mut args = Args::new();
    args.set("value", ArgValue::Text(value));
    disabled_or(item_activation(item, input, source, args))
}

/// The tooltip of a row: the label (and shortcut) of an enabled row,
/// the reason of a disabled one.
fn tooltip_for(label: &str, shortcut: Option<&str>, activation: &Activation) -> String {
    match activation {
        Activation::Disabled { reason } => reason.clone(),
        Activation::Heading => String::new(),
        _ => match shortcut {
            Some(shortcut) if !shortcut.is_empty() => format!("{label} ({shortcut})"),
            _ => label.to_owned(),
        },
    }
}

/// The tooltip of a toolbar button: "Label (Shortcut)" (the
/// `docs/ui/panel-classic.md` rule).
pub(crate) fn button_tooltip(
    label: &str,
    shortcut: Option<&str>,
    activation: &Activation,
) -> String {
    tooltip_for(label, shortcut, activation)
}
