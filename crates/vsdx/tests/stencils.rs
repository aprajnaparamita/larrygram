//! The `.vssx` round-trip of our own stencils (task P4-06, ADR 0034).
//!
//! `stencil_document()` writes to `.vssx` bytes with `write_document`
//! and reads back with `read_document`; the master names, the counts and
//! the geometry row counts must survive. The data is our own work
//! (AGENTS.md section 4): no Microsoft stencil is involved.

use larrygram_model::stencils;
use larrygram_model::{Document, DocumentKind};
use larrygram_vsdx::{Limits, Package, read_document, write_document};

/// The `stencil_document()` written to `.vssx` bytes and read back.
#[allow(clippy::expect_used)] // test helper
fn round_trip() -> Document {
    let document = stencils::stencil_document();
    let bytes = write_document(&document, &Package::new()).expect("write");
    read_document(&bytes, &Limits::DEFAULT).expect("read")
}

/// The read-back master with this `NameU` and its typed geometry rows.
#[allow(clippy::expect_used, clippy::panic)] // test helper
fn geometry_of(document: &Document, name_u: &str) -> Vec<larrygram_model::GeometryRow> {
    let master = document
        .masters
        .iter()
        .find(|master| master.name_u == name_u)
        .unwrap_or_else(|| panic!("no master {name_u}"));
    let shape = master.shapes.first().expect("one shape per master");
    let geometry = document.shape_geometry(*shape).expect("geometry");
    assert_eq!(geometry.paths.len(), 1, "{name_u}: one path");
    geometry.paths[0].rows.clone()
}

#[test]
fn the_stencil_document_round_trips_as_vssx() {
    let back = round_trip();
    assert_eq!(back.kind, DocumentKind::Stencil);
    assert_eq!(
        back.masters.len(),
        21,
        "12 Basic Shapes + 9 Basic Flowchart"
    );
    assert!(back.pages.is_empty(), "a stencil has no pages");
}

#[test]
fn the_basic_shapes_masters_keep_their_names_and_rows() {
    let back = round_trip();
    let stencil = stencils::basic_shapes();
    assert_eq!(stencil.masters.len(), 12);
    for def in &stencil.masters {
        let rows = geometry_of(&back, def.name_u);
        assert_eq!(rows.len(), def.geometry.len(), "{}: rows", def.name_u);
        assert_eq!(rows, def.geometry, "{}: geometry", def.name_u);
    }
}

#[test]
fn the_basic_flowchart_masters_keep_their_names_and_rows() {
    let back = round_trip();
    let stencil = stencils::basic_flowchart();
    assert_eq!(stencil.masters.len(), 9);
    for def in &stencil.masters {
        let rows = geometry_of(&back, def.name_u);
        assert_eq!(rows.len(), def.geometry.len(), "{}: rows", def.name_u);
        assert_eq!(rows, def.geometry, "{}: geometry", def.name_u);
    }
}

#[test]
fn every_master_name_survives_the_round_trip() {
    let back = round_trip();
    let names: Vec<(&str, &str)> = back
        .masters
        .iter()
        .map(|master| (master.name_u.as_str(), master.name.as_str()))
        .collect();
    let expected: Vec<(&str, &str)> = stencils::catalog()
        .into_iter()
        .flat_map(|stencil| stencil.masters)
        .map(|def| (def.name_u, def.name))
        .collect();
    assert_eq!(names, expected);
}

#[test]
fn the_shape_data_and_connection_rows_survive() {
    let back = round_trip();
    for def in stencils::catalog()
        .into_iter()
        .flat_map(|stencil| stencil.masters)
    {
        let master = back
            .masters
            .iter()
            .find(|master| master.name_u == def.name_u)
            .expect("master");
        let shape = back.shape(master.shapes[0]).expect("shape");
        let sections = |name: &str| {
            shape
                .sheet
                .sections
                .iter()
                .find(|section| section.name.as_str() == name)
        };
        if def.connector_1d {
            // The 1-D connector carries neither section (ADR 0084
            // rule 1): no Shape Data rows and no connection points.
            assert!(sections("Property").is_none(), "{}: no Property", def.name_u);
            assert!(
                sections("Connection").is_none(),
                "{}: no Connection",
                def.name_u
            );
            continue;
        }
        let sections = |name: &str| {
            sections(name).unwrap_or_else(|| panic!("{}: no {name} section", def.name_u))
        };
        assert_eq!(
            sections("Property").rows.len(),
            def.shape_data.len(),
            "{}: Shape Data rows",
            def.name_u
        );
        assert_eq!(
            sections("Connection").rows.len(),
            def.connection_points.len(),
            "{}: connection points",
            def.name_u
        );
    }
}
