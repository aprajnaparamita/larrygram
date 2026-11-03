//! Integration tests for [`Document::effective_text`] (task P7-40):
//! an instance shows its own text when it has a `Text` element, else
//! the text of its master chain, through the public API of
//! `larrygram-model`.

// The never-crash lints apply to non-test code only (clippy.toml);
// these helpers are test code and use `expect`/`panic` freely.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use larrygram_model::{
    Document, Master, MasterId, Page, PageId, Shape, ShapeId, TextContent, TextPart,
};

/// Builds a document with one page and one top-level instance shape
/// whose master is `MasterId { index: 0 }`.
fn document_with_instance() -> (Document, ShapeId) {
    let mut doc = Document::new();
    let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
    doc.pages.push(Page::new(PageId { index: 0 }, 1));
    doc.pages[0].shapes.push(id);
    doc.shape_mut(id).expect("shape").master = Some(MasterId { index: 0 });
    (doc, id)
}

/// The text of one literal run.
fn text(literal: &str) -> TextContent {
    TextContent {
        parts: vec![TextPart::Text(literal.to_owned())],
    }
}

/// Adds a master of `index` holding one shape with `content` text and
/// returns the master shape's id.
fn add_master(doc: &mut Document, index: u32, content: Option<TextContent>) -> ShapeId {
    let shape = doc.add_shape(Shape::new(ShapeId { index: 0 }, 100 + index as i32));
    doc.shape_mut(shape).expect("shape").text = content;
    let mut master = Master::new(MasterId { index }, 100 + index as i32);
    master.shapes = vec![shape];
    doc.masters.push(master);
    shape
}

#[test]
fn own_text_wins_over_the_master() {
    let (mut doc, instance) = document_with_instance();
    add_master(&mut doc, 0, Some(text("master")));
    doc.shape_mut(instance).expect("shape").text = Some(text("own"));
    assert_eq!(doc.effective_text(instance), Some(&text("own")));
}

#[test]
fn an_empty_own_text_element_wins_too() {
    // The `Text` element is present but empty: it overrides the
    // master, it is not "no text of its own".
    let (mut doc, instance) = document_with_instance();
    add_master(&mut doc, 0, Some(text("master")));
    doc.shape_mut(instance).expect("shape").text = Some(TextContent::new());
    let effective = doc.effective_text(instance).expect("an empty own text");
    assert!(effective.is_empty());
}

#[test]
fn the_master_text_shows_when_the_instance_has_none() {
    let (mut doc, instance) = document_with_instance();
    let master = add_master(&mut doc, 0, Some(text("master")));
    assert_eq!(doc.shape(instance).expect("shape").text, None);
    assert_eq!(doc.effective_text(instance), Some(&text("master")));
    // The master shape itself shows its own text.
    assert_eq!(doc.effective_text(master), Some(&text("master")));
}

#[test]
fn a_two_rung_chain_reaches_the_grandmaster() {
    // The instance's master shape has no `Text` element of its own and
    // is itself an instance: the walk continues to the grand-master.
    let (mut doc, instance) = document_with_instance();
    let rung = add_master(&mut doc, 0, None);
    doc.shape_mut(rung).expect("shape").master = Some(MasterId { index: 1 });
    add_master(&mut doc, 1, Some(text("deep")));
    assert_eq!(doc.effective_text(instance), Some(&text("deep")));
}

#[test]
fn an_empty_master_text_stops_the_walk() {
    // A `Text` element wins at every rung, empty ones included (the
    // same rule as the instance's own text).
    let (mut doc, instance) = document_with_instance();
    let rung = add_master(&mut doc, 0, Some(TextContent::new()));
    doc.shape_mut(rung).expect("shape").master = Some(MasterId { index: 1 });
    add_master(&mut doc, 1, Some(text("deep")));
    let effective = doc.effective_text(instance).expect("the empty rung");
    assert!(effective.is_empty());
}

#[test]
fn a_cyclic_master_chain_gives_no_text() {
    let (mut doc, instance) = document_with_instance();
    let rung = add_master(&mut doc, 0, None);
    doc.shape_mut(rung).expect("shape").master = Some(MasterId { index: 0 });
    assert_eq!(doc.effective_text(instance), None);
}

#[test]
fn a_missing_master_gives_no_text() {
    let (mut doc, instance) = document_with_instance();
    assert_eq!(doc.effective_text(instance), None);
    doc.shape_mut(instance).expect("shape").master = Some(MasterId { index: 9 });
    assert_eq!(doc.effective_text(instance), None);
}

#[test]
fn a_plain_shape_without_text_has_none() {
    let mut doc = Document::new();
    let id = doc.add_shape(Shape::new(ShapeId { index: 0 }, 1));
    doc.pages.push(Page::new(PageId { index: 0 }, 1));
    doc.pages[0].shapes.push(id);
    assert_eq!(doc.effective_text(id), None);
}

#[test]
fn a_stale_shape_id_gives_none() {
    let doc = Document::new();
    assert_eq!(doc.effective_text(ShapeId { index: 99 }), None);
}
