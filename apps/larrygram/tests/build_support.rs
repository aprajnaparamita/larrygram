//! The build script's `.ico` builder, unit tested (task R11 goal 3).
//!
//! `build.rs` includes `build_support/icon.rs` to make the Windows
//! icon resource at build time; this test target includes the same
//! file so the container format is tested, because a build script's
//! own `#[cfg(test)]` code never runs.

// The never-crash lints apply to non-test code only (AGENTS.md
// section 6); the shared module is lint-clean without them.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "../build_support/icon.rs"]
mod icon;
