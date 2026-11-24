//! Checks the potion-2M model of the example firmware end to end.

use esp_agent::{Embedder, EmbedderConfig, Model, WordPieceTokenizer, cosine_similarity, model};

static POTION: Model = model!("../../examples/semantic-search/model");

#[test]
fn potion_vectors_have_the_model_dimension_and_unit_length() {
    let vector = Embedder::new(POTION).embed("The quick brown fox jumps over the lazy dog.");

    assert_eq!(POTION.dim(), 64);
    assert_eq!(POTION.vocab_size(), 29528);
    assert_eq!(vector.len(), POTION.dim());
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
}

#[test]
fn potion_ranks_paraphrases_above_unrelated_sentences() {
    let embedder = Embedder::new(POTION);
    let fox = embedder.embed("The quick brown fox jumps over the lazy dog.");
    let hound = embedder.embed("A fast auburn fox leaps above a sleepy hound.");
    let revenue = embedder.embed("Quarterly revenue grew 12% year over year.");

    assert!(cosine_similarity(&fox, &hound) > cosine_similarity(&fox, &revenue));
}

#[test]
fn public_types_implement_debug() {
    fn assert_debug<T: core::fmt::Debug>() {}

    assert_debug::<Embedder>();
    assert_debug::<EmbedderConfig>();
    assert_debug::<Model>();
    assert_debug::<WordPieceTokenizer>();
}
