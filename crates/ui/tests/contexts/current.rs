//! `contexts::current[_with]` against one document per context.

use larrygram_ui::contexts::{current, current_with};

use super::documents;

#[test]
fn current_matches_every_document() {
    for case in documents::all() {
        assert_eq!(
            current_with(&case.app, case.ui),
            case.context,
            "current context for {:?}",
            case.context
        );
    }
}

#[test]
fn current_without_ui_state_covers_the_drawing_contexts() {
    for case in documents::all() {
        if case.ui.text_editing || case.ui.window != larrygram_ui::contexts::WindowMode::Drawing {
            assert_ne!(
                current(&case.app),
                case.context,
                "the UI-only state must not be guessed by current(): {:?}",
                case.context
            );
            continue;
        }
        assert_eq!(
            current(&case.app),
            case.context,
            "current context for {:?}",
            case.context
        );
    }
}
