//! What one layout item must do when it is activated (ADR 0031, ADR
//! 0065): the expectation computed from the committed data and the
//! builtin registry, mirroring the two widget resolvers
//! (`panel_classic::rows` and `panel_modern::common`).
//!
//! The sweep tests drive every menu, dropdown and context menu of the
//! committed layouts against these expectations: an entry the model
//! says dispatches must dispatch exactly its `command`, and one it says
//! disables must render disabled with the reason tooltip.

use larrygram_commands::app::AppCtx;
use larrygram_commands::{ArgValue, Args, CommandId, CommandStatus, Registry};
use larrygram_ui_spec::embedded::is_out_of_scope;
use larrygram_ui_spec::v2::{Item, ItemKind};

/// Tooltip of a control whose command has no handler (the Phase 4
/// tooltip, ADR 0031).
pub const NOT_YET: &str = "Not yet implemented";

/// Tooltip of a control whose Fluent id is out of scope (ADR 0031).
pub const OUT_OF_SCOPE: &str = "Out of scope for this run";

/// The disabled label of an empty run-time list (`docs/ui/layout-format.md`).
pub const NONE_ROW: &str = "(none)";

/// The accessible name of a dropdown arrow (ADR 0031): the arrow is a
/// painted mark and the tooltip text doubles as its name.
pub const DROPDOWN_TIP: &str = "More items";

/// What activating one drawn entry does.
#[derive(Debug, Clone, PartialEq)]
pub enum Expect {
    /// Dispatch this command; `value` is the option text when the row
    /// is a Panel Classic option row (the `value` argument).
    Dispatch {
        /// The registry id the entry must dispatch.
        command: String,
        /// The `value` argument, where the surface carries one.
        value: Option<String>,
    },
    /// Render disabled with this tooltip (empty: no tooltip).
    Disabled {
        /// The tooltip text the widget must show.
        reason: String,
    },
    /// Open a submenu; its entries follow.
    Submenu(Vec<Entry>),
    /// A heading row: drawn, not clickable.
    Heading,
    /// A View > Toolbars row: flips one toolbar's visibility and
    /// dispatches `command` when it has one (`Activation::ToolbarToggle`).
    Toggle {
        /// The command the row dispatches, when it resolves.
        command: Option<String>,
    },
}

/// One drawn entry: its label and what activation does.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The accessible name the widget draws.
    pub label: String,
    /// What activation does.
    pub expect: Expect,
}

/// The activation of one plain item through the registry (the
/// `command_activation` rules both widgets share).
pub fn resolve(item: &Item, registry: &Registry, app: &AppCtx, value: Option<String>) -> Expect {
    if item
        .fluent
        .as_ref()
        .is_some_and(|fluent| is_out_of_scope(fluent.as_str()))
    {
        return Expect::Disabled {
            reason: OUT_OF_SCOPE.to_owned(),
        };
    }
    let Some(command) = &item.command else {
        return Expect::Disabled {
            reason: NOT_YET.to_owned(),
        };
    };
    let Ok(id) = CommandId::try_from(command) else {
        return Expect::Disabled {
            reason: NOT_YET.to_owned(),
        };
    };
    if registry.status(&id) != CommandStatus::Implemented {
        return Expect::Disabled {
            reason: NOT_YET.to_owned(),
        };
    }
    let enabled = registry.enabled(&id, app);
    if !enabled.is_enabled() {
        return Expect::Disabled {
            reason: enabled.reason.unwrap_or_else(|| NOT_YET.to_owned()),
        };
    }
    Expect::Dispatch {
        command: id.as_str().to_owned(),
        value,
    }
}

/// The args one [`Expect::Dispatch`] carries.
pub fn args_of(expect: &Expect) -> Args {
    let mut args = Args::new();
    if let Expect::Dispatch {
        value: Some(value), ..
    } = expect
    {
        args.set("value", ArgValue::Text(value.clone()));
    }
    args
}

/// The menu whose rows toggle toolbar visibility (`rows::rows` marks
/// its children `in_toolbars_menu`).
pub const TOOLBARS_MENU: &str = "Toolbars";

/// The rows of the View > Toolbars submenu: each flips one toolbar's
/// visibility and dispatches its command when it resolves.
pub fn classic_toolbars_level(items: &[Item], registry: &Registry, app: &AppCtx) -> Vec<Entry> {
    items
        .iter()
        .filter(|item| !item.hidden)
        .map(|item| Entry {
            label: item.label.clone().unwrap_or_default(),
            expect: Expect::Toggle {
                command: match resolve(item, registry, app, None) {
                    Expect::Dispatch { command, .. } => Some(command),
                    _ => None,
                },
            },
        })
        .collect()
}

/// The label of the "(none)" row of an empty run-time list.
fn none_row(classic: bool) -> Entry {
    Entry {
        label: NONE_ROW.to_owned(),
        expect: Expect::Disabled {
            // The Panel Classic rows carry an empty reason (no
            // tooltip); the Panel Modern rows hover "(none)".
            reason: if classic {
                String::new()
            } else {
                NONE_ROW.to_owned()
            },
        },
    }
}

/// The entries one **Panel Classic** menu, popup or context-menu level
/// draws for `items` (`rows::rows`), with empty run-time lists.
///
/// `submenus` keeps nested rows as [`Expect::Submenu`]; the caller
/// walks them level by level.
pub fn classic_level(items: &[Item], registry: &Registry, app: &AppCtx) -> Vec<Entry> {
    let mut out = Vec::new();
    for item in items {
        if item.hidden {
            continue;
        }
        match item.kind {
            ItemKind::List => out.push(none_row(true)),
            ItemKind::Options => {
                for option in &item.options {
                    let expect = match resolve(item, registry, app, Some(option.clone())) {
                        Expect::Dispatch { command, value } => Expect::Dispatch { command, value },
                        other => other,
                    };
                    out.push(Entry {
                        label: option.clone(),
                        expect,
                    });
                }
            }
            ItemKind::Heading => out.push(Entry {
                label: item.label.clone().unwrap_or_default(),
                expect: Expect::Heading,
            }),
            _ if !item.items.is_empty() => {
                let children = if item.label.as_deref() == Some(TOOLBARS_MENU) {
                    // The View > Toolbars rows toggle visibility
                    // instead of dispatching (`rows::rows` marks the
                    // level `in_toolbars_menu`).
                    classic_toolbars_level(&item.items, registry, app)
                } else {
                    classic_level(&item.items, registry, app)
                };
                out.push(Entry {
                    label: item.label.clone().unwrap_or_default(),
                    expect: Expect::Submenu(children),
                });
            }
            _ => out.push(Entry {
                label: item.label.clone().unwrap_or_default(),
                expect: resolve(item, registry, app, None),
            }),
        }
    }
    out
}

/// The entries one **Panel Modern** dropdown, split or gallery popup
/// draws for `items` (`panel_modern::controls::show_menu`), with empty
/// run-time lists.
///
/// Panel Modern option rows dispatch the item's command with no
/// `value` argument (task P7-09's "not yours" list records the gap),
/// so the expectation carries no value either.
pub fn modern_entries(items: &[Item], registry: &Registry, app: &AppCtx) -> Vec<Entry> {
    let mut out = Vec::new();
    for item in items {
        if item.hidden {
            continue;
        }
        match item.kind {
            ItemKind::Heading => out.push(Entry {
                label: item.label.clone().unwrap_or_default(),
                expect: Expect::Heading,
            }),
            ItemKind::List => out.push(none_row(false)),
            ItemKind::Options => {
                for option in &item.options {
                    out.push(Entry {
                        label: option.clone(),
                        expect: resolve(item, registry, app, None),
                    });
                }
            }
            ItemKind::Submenu
            | ItemKind::Dropdown
            | ItemKind::Gallery
            | ItemKind::Split
            | ItemKind::SplitRecent
                if !item.items.is_empty() =>
            {
                out.push(Entry {
                    label: item.label.clone().unwrap_or_default(),
                    expect: Expect::Submenu(modern_entries(&item.items, registry, app)),
                });
            }
            _ => out.push(Entry {
                label: item.label.clone().unwrap_or_default(),
                expect: resolve(item, registry, app, None),
            }),
        }
    }
    out
}

/// The popup entries one Panel Modern **control** opens: its `options`
/// (a combo), or its `items` (and a gallery's `list` rows).
pub fn modern_control_entries(item: &Item, registry: &Registry, app: &AppCtx) -> Vec<Entry> {
    match item.kind {
        // A combo lists its `options`, else its `items`
        // (`panel_modern::controls::show_options`).
        ItemKind::Combo if !item.options.is_empty() => item
            .options
            .iter()
            .map(|option| Entry {
                label: option.clone(),
                expect: resolve(item, registry, app, None),
            })
            .collect(),
        // A gallery draws its `list` rows above its `items`; an empty
        // run-time list shows the "(none)" placeholder.
        ItemKind::Gallery => {
            let mut out = vec![none_row(false)];
            out.extend(modern_entries(&item.items, registry, app));
            out
        }
        // A dropdown or split opens its `items` (its `options` are not
        // drawn, as `show_menu` reads `items` only).
        _ => modern_entries(&item.items, registry, app),
    }
}

/// The activation of a Panel Modern control button itself.
pub fn modern_control(item: &Item, registry: &Registry, app: &AppCtx) -> Expect {
    resolve(item, registry, app, None)
}
