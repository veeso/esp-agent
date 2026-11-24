//! Checks the store side of a knowledge base through a real model folder.

use esp_agent::{Embedder, Knowledge, Model, model};

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");

#[test]
fn extend_inserts_every_text() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY_2D));
    knowledge.extend([("hello", 'a'), ("fox", 'b')]);

    assert_eq!(knowledge.len(), 2);
    assert!(!knowledge.is_empty());
    assert_eq!(knowledge.index().dim(), 2);
}

#[test]
fn text_returns_the_original_text() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY_2D));
    knowledge.insert("hello", ());
    knowledge.insert("fox", ());

    assert_eq!(knowledge.text(0), Some("hello"));
    assert_eq!(knowledge.text(knowledge.len()), None);
}

#[test]
fn with_capacity_starts_empty() {
    let knowledge = Knowledge::<()>::with_capacity(Embedder::new(TINY_2D), 4);

    assert_eq!(knowledge.len(), 0);
    assert!(knowledge.is_empty());
}
