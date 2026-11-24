//! Checks that a retriever finds stored texts through a real model folder.

use esp_agent::{Embedder, Model, Retriever, Scratch, model};

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");
static TINY_3D: Model = model!("tests/fixtures/tiny-3d");

#[derive(Debug, PartialEq)]
struct Doc {
    id: u32,
}

fn three_docs() -> Retriever<Doc> {
    let mut retriever = Retriever::new(Embedder::new(TINY_2D));
    retriever.insert(&String::from("hello"), Doc { id: 1 });
    retriever.insert("fox", Doc { id: 2 });
    retriever.insert("world", Doc { id: 3 });
    retriever
}

#[test]
fn retriever_returns_closest_texts_first() {
    let retriever = three_docs();
    let hits = retriever.search("hello", usize::MAX);

    let docs: Vec<_> = hits.iter().map(|hit| (hit.text, hit.payload.id)).collect();
    let scores: Vec<f32> = hits.iter().map(|hit| hit.score).collect();
    assert_eq!(docs, [("hello", 1), ("fox", 2), ("world", 3)]);
    let expected = [1.0, core::f32::consts::FRAC_1_SQRT_2, 0.0];
    for (score, expected) in scores.iter().zip(expected) {
        assert!((score - expected).abs() < 1e-6, "score was {score}");
    }
}

#[test]
fn query_without_known_words_returns_hits_in_insertion_order() {
    let retriever = three_docs();
    let hits = retriever.search("xyz", 2);

    let indices: Vec<usize> = hits.iter().map(|hit| hit.index).collect();
    let scores: Vec<f32> = hits.iter().map(|hit| hit.score).collect();
    assert_eq!(indices, [0, 1]);
    assert_eq!(scores, [0.0, 0.0]);
}

#[test]
fn extend_inserts_every_text() {
    let mut retriever = Retriever::new(Embedder::new(TINY_2D));
    retriever.extend([("hello", 'a'), ("fox", 'b')]);

    assert_eq!(retriever.len(), 2);
    assert!(!retriever.is_empty());
    assert_eq!(retriever.index().dim(), 2);
    assert_eq!(retriever.search("fox", 1)[0].payload, &'b');
}

#[test]
fn one_scratch_serves_retrievers_of_different_dimensions() {
    let mut flat = Retriever::new(Embedder::new(TINY_2D));
    flat.insert("hello", ());
    let mut deep = Retriever::new(Embedder::new(TINY_3D));
    deep.insert("hello", ());
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();

    for retriever in [&flat, &deep, &flat] {
        retriever.search_with("hello", 1, &mut scratch, &mut hits);
        assert_eq!(hits.len(), 1);
        let score = hits[0].score;
        assert!((score - 1.0).abs() < 1e-6, "score was {score}");
    }
}

#[test]
fn empty_search_replaces_previous_hits() {
    let retriever = three_docs();
    let empty = Retriever::new(Embedder::new(TINY_2D));
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();
    for (next, k) in [(&retriever, 0), (&empty, 1)] {
        retriever.search_with("hello", 1, &mut scratch, &mut hits);
        assert_eq!(hits.len(), 1);
        next.search_with("hello", k, &mut scratch, &mut hits);
        assert!(hits.is_empty());
    }
}

#[test]
fn reused_hits_can_read_text_owned_by_the_retriever() {
    let retriever = three_docs();
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();
    retriever.search_with("hello", 1, &mut scratch, &mut hits);
    assert_eq!(retriever.text(hits[0].index), Some("hello"));
    assert_eq!(retriever.text(retriever.len()), None);
}
