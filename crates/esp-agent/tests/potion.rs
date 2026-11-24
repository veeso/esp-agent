//! Checks the potion-2M model of the example firmware end to end.

use esp_agent::{
    Agent, AgentError, Embedder, EmbedderConfig, Knowledge, Model, Scratch, WordPieceTokenizer,
    cosine_similarity, model,
};

#[path = "../../../examples/semantic-search/src/corpus.rs"]
mod corpus;
#[path = "../../../examples/semantic-search/src/results.rs"]
mod results;

use corpus::{CORPUS, TOP_K};

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
fn potion_embed_with_matches_embed() {
    let embedder = Embedder::new(POTION);
    let mut scratch = Scratch::new();
    let mut out = vec![0.0; POTION.dim()];

    for sentence in CORPUS {
        embedder.embed_with(sentence, &mut scratch, &mut out);
        assert_eq!(out, embedder.embed(sentence), "{sentence}");
    }
}

#[test]
fn public_types_implement_debug() {
    fn assert_debug<T: core::fmt::Debug>() {}

    assert_debug::<Agent<'static, ()>>();
    assert_debug::<AgentError>();
    assert_debug::<Embedder>();
    assert_debug::<EmbedderConfig>();
    assert_debug::<Knowledge<()>>();
    assert_debug::<Model>();
    assert_debug::<WordPieceTokenizer>();
}

#[test]
fn potion_matches_the_firmware_ranking() {
    let mut knowledge = Knowledge::new(Embedder::new(POTION));
    knowledge.extend(CORPUS.map(|text| (text, ())));
    let agent = Agent::new(POTION, &knowledge).unwrap();
    let mut scratch = Scratch::new();

    for (question, expected) in [
        (
            "Which city is the capital of France?",
            [(5, "0.763"), (6, "0.123"), (2, "0.017")],
        ),
        ("kung sea", [(0, "0.128"), (6, "0.128"), (5, "0.093")]),
        (
            "reform clubhouse",
            [(4, "0.054"), (2, "0.051"), (5, "0.035")],
        ),
    ] {
        let mut hits = Vec::with_capacity(TOP_K);
        agent.prompt_with(question, TOP_K, &mut scratch, &mut hits);
        let mut lines = Vec::new();
        results::render_hits(&hits, &CORPUS, |line| lines.push(line.to_string()));
        let expected_lines: Vec<_> = (1..)
            .zip(expected)
            .map(|(rank, (index, score))| {
                let text = CORPUS[index];
                format!("  {rank}. [{score}] {text}")
            })
            .collect();
        assert_eq!(lines, expected_lines, "query was {question}");
    }
}
