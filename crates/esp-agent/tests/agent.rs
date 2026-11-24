//! Checks that an agent answers prompts from a knowledge store through a real model folder.

use esp_agent::{Agent, AgentError, Embedder, Knowledge, Model, Scratch, model};

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");
static TINY_3D: Model = model!("tests/fixtures/tiny-3d");

#[derive(Debug, PartialEq)]
struct Doc {
    id: u32,
}

fn three_docs() -> Knowledge<Doc> {
    let mut knowledge = Knowledge::new(Embedder::new(TINY_2D));
    knowledge.insert(&String::from("hello"), Doc { id: 1 });
    knowledge.insert("fox", Doc { id: 2 });
    knowledge.insert("world", Doc { id: 3 });
    knowledge
}

#[test]
fn agent_returns_closest_texts_first() {
    let knowledge = three_docs();
    let agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let hits = agent.prompt("hello", usize::MAX);

    let docs: Vec<_> = hits.iter().map(|hit| (hit.text, hit.payload.id)).collect();
    let scores: Vec<f32> = hits.iter().map(|hit| hit.score).collect();
    assert_eq!(docs, [("hello", 1), ("fox", 2), ("world", 3)]);
    let expected = [1.0, core::f32::consts::FRAC_1_SQRT_2, 0.0];
    for (score, expected) in scores.iter().zip(expected) {
        assert!((score - expected).abs() < 1e-6, "score was {score}");
    }
}

#[test]
fn prompt_without_known_words_returns_hits_in_insertion_order() {
    let knowledge = three_docs();
    let agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let hits = agent.prompt("xyz", 2);

    let indices: Vec<usize> = hits.iter().map(|hit| hit.index).collect();
    let scores: Vec<f32> = hits.iter().map(|hit| hit.score).collect();
    assert_eq!(indices, [0, 1]);
    assert_eq!(scores, [0.0, 0.0]);
}

#[test]
fn one_scratch_serves_agents_of_different_dimensions() {
    let mut flat = Knowledge::new(Embedder::new(TINY_2D));
    flat.insert("hello", ());
    let mut deep = Knowledge::new(Embedder::new(TINY_3D));
    deep.insert("hello", ());
    let flat_agent = Agent::new(TINY_2D, &flat).unwrap();
    let deep_agent = Agent::new(TINY_3D, &deep).unwrap();
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();

    for agent in [&flat_agent, &deep_agent, &flat_agent] {
        agent.prompt_with("hello", 1, &mut scratch, &mut hits);
        assert_eq!(hits.len(), 1);
        let score = hits[0].score;
        assert!((score - 1.0).abs() < 1e-6, "score was {score}");
    }
}

#[test]
fn empty_prompt_results_replace_previous_hits() {
    let knowledge = three_docs();
    let empty = Knowledge::<Doc>::new(Embedder::new(TINY_2D));
    let full_agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let empty_agent = Agent::new(TINY_2D, &empty).unwrap();
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();
    for (next, k) in [(&full_agent, 0), (&empty_agent, 1)] {
        full_agent.prompt_with("hello", 1, &mut scratch, &mut hits);
        assert_eq!(hits.len(), 1);
        next.prompt_with("hello", k, &mut scratch, &mut hits);
        assert!(hits.is_empty());
    }
}

#[test]
fn reused_hits_can_read_text_owned_by_the_store() {
    let knowledge = three_docs();
    let agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let mut scratch = Scratch::new();
    let mut hits = Vec::new();
    agent.prompt_with("hello", 1, &mut scratch, &mut hits);

    assert_eq!(agent.knowledge().text(hits[0].index), Some("hello"));
    assert_eq!(agent.knowledge().text(agent.knowledge().len()), None);
}

#[test]
fn agent_rejects_a_store_of_another_model() {
    let mut store = Knowledge::new(Embedder::new(TINY_3D));
    store.insert("hello", ());

    let result = Agent::new(TINY_2D, &store);

    assert_eq!(result.err(), Some(AgentError::ModelMismatch));
}

#[test]
fn agent_rejects_an_empty_store_of_another_model() {
    let store = Knowledge::<()>::new(Embedder::new(TINY_3D));

    let result = Agent::new(TINY_2D, &store);

    assert_eq!(result.err(), Some(AgentError::ModelMismatch));
}

#[test]
fn prompt_over_an_empty_store_returns_no_hits() {
    let store = Knowledge::<()>::new(Embedder::new(TINY_2D));
    let agent = Agent::new(TINY_2D, &store).unwrap();

    assert!(agent.prompt("hello", 3).is_empty());
}

#[test]
fn hits_outlive_the_agent() {
    let knowledge = three_docs();
    let hits = {
        let agent = Agent::new(TINY_2D, &knowledge).unwrap();
        agent.prompt("hello", 1)
    };

    assert_eq!(hits[0].text, "hello");
}

#[test]
fn model_mismatch_displays_a_message() {
    assert!(!AgentError::ModelMismatch.to_string().is_empty());
}
