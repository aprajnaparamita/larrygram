//! Selection contexts (task P7-08, ADR 0064).
//!
//! One test document per context (built in code, no corpus files), the
//! hit-target-to-menu mapping for both layouts, the enablement
//! cross-check against `ui-spec/contexts.yaml`, and the toggle
//! defaults against `Settings` and `ViewState`.

#![allow(dead_code)]
// The never-crash lints apply to non-test code only (clippy.toml).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod crosscheck;
mod current;
mod documents;
mod menus;
mod toggles;

/// The committed `contexts.yaml`.
pub const CONTEXTS_YAML: &str = include_str!("../../../../ui-spec/contexts.yaml");
/// The committed Panel Classic layout.
pub const CLASSIC_YAML: &str = include_str!("../../../../ui-spec/layouts/panel-classic.yaml");
/// The committed Panel Modern layout.
pub const MODERN_YAML: &str = include_str!("../../../../ui-spec/layouts/panel-modern.yaml");
