//! Checks the embedded potion-2M model end to end.

use esp_local_agent::{DIM, cosine_similarity, embedder};

#[test]
fn potion_vectors_have_the_model_dimension_and_unit_length() {
    let vector = embedder().embed("The quick brown fox jumps over the lazy dog.");

    assert_eq!(vector.len(), DIM);
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-5, "norm was {norm}");
}

#[test]
fn potion_ranks_paraphrases_above_unrelated_sentences() {
    let embedder = embedder();
    let fox = embedder.embed("The quick brown fox jumps over the lazy dog.");
    let hound = embedder.embed("A fast auburn fox leaps above a sleepy hound.");
    let revenue = embedder.embed("Quarterly revenue grew 12% year over year.");

    assert!(cosine_similarity(&fox, &hound) > cosine_similarity(&fox, &revenue));
}

#[test]
fn module_paths_remain_available() {
    use esp_local_agent::embedder::{
        DEFAULT_MAX_LENGTH, EmbedderError, StaticEmbedder, cosine_similarity,
    };
    use esp_local_agent::tokenizer::{Vocab, WordPieceTokenizer, normalize};

    let tokenizer: WordPieceTokenizer = esp_local_agent::tokenizer();
    let _: Vocab = Vocab::new(&[0; 4]);
    let result: Result<StaticEmbedder, EmbedderError> = StaticEmbedder::new(tokenizer, &[], 0, 2);
    assert_eq!(result.unwrap().dim(), 2);
    assert_eq!(normalize("CAFÉ"), "cafe");
    assert_eq!(esp_local_agent::normalize("CAFÉ"), "cafe");
    assert_eq!(DEFAULT_MAX_LENGTH, esp_local_agent::DEFAULT_MAX_LENGTH);
    assert_eq!(cosine_similarity(&[1.0], &[1.0]), 1.0);
}
