//! Toggle defaults: `on_by_default` against `Settings` and `ViewState`
//! (task P7-08, ADR 0064).

use larrygram_commands::app::{Settings, ViewState};
use larrygram_ui_spec::v2::Contexts;

use super::CONTEXTS_YAML;

fn contexts() -> Contexts {
    serde_saphyr::from_str(CONTEXTS_YAML).expect("the committed contexts table decodes")
}

#[test]
fn rulers_and_grid_defaults_match_the_settings() {
    let settings = Settings::default();
    let view = ViewState::from_settings(&settings);
    let table = contexts();
    assert_eq!(
        table.commands["view.rulers"].on_by_default,
        Some(settings.show_rulers)
    );
    assert_eq!(view.show_rulers, settings.show_rulers);
    // Known difference: `view.toggle_grid` has no `on_by_default` row,
    // while `Settings::show_grid` (and `ViewState`) default to on.
    assert_eq!(table.commands["view.toggle_grid"].on_by_default, None);
    assert_eq!(view.show_grid, settings.show_grid);
}

#[test]
fn every_on_by_default_toggle_is_named() {
    let table = contexts();
    let mut toggles: Vec<&str> = table
        .commands
        .iter()
        .filter(|(_, availability)| availability.on_by_default == Some(true))
        .map(|(id, _)| id.as_str())
        .collect();
    toggles.sort_unstable();
    // Settings and ViewState only carry the grid and ruler defaults; the
    // other toggles have no state yet (listed in the task Result).
    let state_backed = ["view.rulers", "view.toggle_grid"];
    for id in toggles {
        if state_backed.contains(&id) {
            continue;
        }
        assert!(
            table.commands.contains_key(id),
            "{id} is a toggle with no state or command row"
        );
    }
}
