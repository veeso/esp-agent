//! Checks that a retriever finds stored texts through a real model folder.

use esp_agent::{Embedder, Model, Retriever, model};

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");

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
