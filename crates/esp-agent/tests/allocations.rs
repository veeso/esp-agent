//! Checks the allocation contracts of the reusable-buffer and lookup paths.

#[path = "support/allocation_counter.rs"]
mod allocation_counter;

use allocation_counter::allocations_during;
use esp_agent::{
    Agent, DynamicTarget, DynamicTargetCatalog, Embedder, Index, Knowledge, Model, Scratch, model,
};

#[test]
fn counter_sees_allocations() {
    assert_eq!(
        allocations_during(|| drop(std::hint::black_box(vec![1u8; 16]))),
        1
    );
}

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");

#[test]
fn encode_with_a_warm_scratch_does_not_allocate() {
    let tokenizer = Embedder::new(TINY_2D).tokenizer();
    let mut scratch = Scratch::new();
    tokenizer.encode_with("hello world fox dog", &mut scratch);

    let allocations = allocations_during(|| {
        std::hint::black_box(tokenizer.encode_with("dog hello", &mut scratch));
    });

    assert_eq!(allocations, 0);
}

#[test]
fn embed_with_a_warm_scratch_does_not_allocate() {
    let embedder = Embedder::new(TINY_2D);
    let mut scratch = Scratch::new();
    let mut out = [0.0f32; 2];
    embedder.embed_with("hello world fox dog", &mut scratch, &mut out);

    let allocations = allocations_during(|| {
        embedder.embed_with("fox hello", &mut scratch, &mut out);
    });

    assert_eq!(allocations, 0);
}

#[test]
fn search_into_with_enough_capacity_does_not_allocate() {
    let mut index = Index::new(2);
    for (payload, vector) in [[1.0, 0.0], [0.0, 1.0], [1.0, 1.0], [-1.0, 0.0], [1.0, -1.0]]
        .iter()
        .enumerate()
    {
        index.insert(vector, payload as u8);
    }
    let mut hits = Vec::with_capacity(3);

    let allocations = allocations_during(|| {
        index.search_into(&[1.0, 0.0], 3, &mut hits);
    });

    assert_eq!(allocations, 0);
    assert_eq!(hits.len(), 3);
}

#[test]
fn agent_prompt_with_a_warm_scratch_does_not_allocate() {
    let mut knowledge = Knowledge::with_capacity(Embedder::new(TINY_2D), 3);
    knowledge.extend([("hello", ()), ("world", ()), ("dog", ())]);
    let agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let mut scratch = Scratch::new();
    let mut hits = Vec::with_capacity(3);
    agent.prompt_with("hello world fox dog", 3, &mut scratch, &mut hits);

    let allocations = allocations_during(|| {
        agent.prompt_with("fox dog", 3, &mut scratch, &mut hits);
    });

    assert_eq!(allocations, 0);
    assert_eq!(hits.len(), 3);
}

#[test]
fn repeated_unicode_embedding_and_search_do_not_allocate() {
    let embedder = Embedder::new(TINY_2D);
    let expected = embedder.embed("hello");
    let mut knowledge = Knowledge::new(embedder);
    knowledge.insert("hello", ());
    let agent = Agent::new(TINY_2D, &knowledge).unwrap();
    let mut out = [0.0; 2];
    let mut hits = Vec::with_capacity(1);
    let mut scratch = Scratch::new();
    let text = "hel\u{0301}\u{0301}\u{0301}\u{0301}lo \u{302e}\u{1b44}";
    agent.prompt_with(text, 1, &mut scratch, &mut hits);

    let allocations = allocations_during(|| {
        for _ in 0..2 {
            knowledge
                .embedder()
                .embed_with(text, &mut scratch, &mut out);
            agent.prompt_with(text, 1, &mut scratch, &mut hits);
        }
    });
    assert_eq!(allocations, 0);
    assert_eq!(out.as_slice(), expected);
    assert_eq!(hits[0].index, 0);
}

#[test]
fn search_into_ranks_increasing_scores_in_reused_storage() {
    let mut index = Index::new(2);
    for i in 0..1024 {
        index.insert(&[i as f32 / 1024.0, 1.0], ());
    }
    let mut hits = Vec::with_capacity(index.len());
    for k in [1024, 513, 2] {
        let allocations = allocations_during(|| index.search_into(&[1.0, 0.0], k, &mut hits));
        assert_eq!(allocations, 0);
        assert!(hits.iter().map(|hit| hit.index).eq((1024 - k..1024).rev()));
    }
}

#[test]
fn command_matcher_borrows_input_without_heap_allocation() {
    use esp_agent::{TargetSelector, TurnCommands, vocabulary};
    #[derive(TurnCommands)]
    enum Op {
        #[command("read {target} at {sample}")]
        Read {
            sample: u8,
        },
        Stop,
    }
    let line = String::from("read météo at 7");
    assert_eq!(
        allocations_during(|| {
            let cmd = Op::parse_command(std::hint::black_box(&line)).unwrap();
            assert!(matches!(cmd.action, Op::Read { sample: 7 }));
            let TargetSelector::Explicit(target) = cmd.target else {
                panic!()
            };
            assert_eq!(target.as_ptr(), line[5..].as_ptr());
        }),
        0
    );

    let italian = vocabulary! {
        Op::Read { sample: u8 } => explicit ["leggi {target} a {sample}"];
        Op::Stop => previous ["fermati"];
    };
    let line = String::from("leggi météo a 7");
    assert_eq!(
        allocations_during(|| {
            let cmd = italian.parse(std::hint::black_box("Fermati!")).unwrap();
            assert!(matches!(cmd.action, Op::Stop));
            assert_eq!(cmd.target, TargetSelector::Previous);

            let cmd = italian.parse(std::hint::black_box(&line)).unwrap();
            assert!(matches!(cmd.action, Op::Read { sample: 7 }));
            let TargetSelector::Explicit(target) = cmd.target else {
                panic!("expected an explicit target")
            };
            assert_eq!(target, "météo");
            assert_eq!(target.as_ptr(), line[6..].as_ptr());
        }),
        0
    );
}

#[test]
fn dynamic_catalog_lookups_allocate_only_returned_target_lists() {
    use std::hint::black_box;

    let mut catalog = DynamicTargetCatalog::new();
    catalog
        .insert(DynamicTarget::new(1, "desk lamp").with_replies(["desk one"]))
        .unwrap();
    catalog.insert(DynamicTarget::new(2, "floor lamp")).unwrap();
    catalog.insert_choice("lamp", [1, 2]).unwrap();
    catalog.insert_group(["all lights"], [1, 2]).unwrap();

    assert_eq!(
        allocations_during(|| {
            black_box(catalog.contains_selector(black_box(" THE LAMP?! ")));
        }),
        0,
        "contains_selector"
    );
    assert_eq!(
        allocations_during(|| {
            black_box(catalog.matches_target(1, black_box(" THE DESK ONE?! ")));
        }),
        0,
        "matches_target"
    );
    assert_eq!(
        allocations_during(|| {
            black_box(catalog.name(black_box(1)));
        }),
        0,
        "name"
    );

    for (selector, expected) in [
        (" THE UNKNOWN LAMP?! ", 0),
        (" THE DESK LAMP?! ", 1),
        (" THE LAMP?! ", 1),
        (" THE ALL LIGHTS?! ", 1),
    ] {
        assert_eq!(
            allocations_during(|| {
                black_box(catalog.resolve_targets(black_box(selector)));
            }),
            expected,
            "resolve_targets({selector:?})"
        );
    }
}

#[test]
fn runtime_vocabulary_parse_does_not_allocate() {
    use esp_agent::{RuntimeVocabulary, TargetSelector, TurnCommands};
    #[derive(TurnCommands)]
    enum Op {
        #[command("stop {target}")]
        Stop,
        #[command("read {target} at {sample}")]
        Read { sample: u8 },
    }
    let mut words = RuntimeVocabulary::<Op>::new();
    words.insert_explicit("Stop", "spegni {target}").unwrap();
    words.insert_previous("Stop", "spegnila").unwrap();
    words
        .insert_explicit("Read", "leggi {target} a {sample}")
        .unwrap();
    let line = String::from("leggi météo a 7");

    let allocations = allocations_during(|| {
        let cmd = words.parse(std::hint::black_box("spegni Luna")).unwrap();
        assert!(matches!(cmd.action, Op::Stop));
        assert_eq!(cmd.target, TargetSelector::Explicit("Luna"));

        let cmd = words.parse(std::hint::black_box("Spegnila!")).unwrap();
        assert_eq!(cmd.target, TargetSelector::Previous);

        let cmd = words.parse(std::hint::black_box(&line)).unwrap();
        assert!(matches!(cmd.action, Op::Read { sample: 7 }));
        let TargetSelector::Explicit(target) = cmd.target else {
            panic!("expected an explicit target")
        };
        assert_eq!(target.as_ptr(), line[6..].as_ptr());
        assert!(words.parse(std::hint::black_box("niente")).is_none());
    });

    assert_eq!(allocations, 0);
}

#[test]
fn refusing_a_phrase_of_captures_stops_at_the_capture_limit() {
    use esp_agent::{RuntimeVocabulary, TurnCommands, VocabularyError};
    #[derive(TurnCommands)]
    #[expect(dead_code, reason = "the test only offers phrases for the field")]
    enum Op {
        #[command("set {target} {a}")]
        Set { a: u8 },
    }
    let mut words = RuntimeVocabulary::<Op>::new();
    // As long as the default limits allow, and nothing but captures.
    let phrase = "{a}x".repeat(4096);

    let allocations = allocations_during(|| {
        assert_eq!(
            words.insert_previous("Set", &phrase),
            Err(VocabularyError::TooManyCaptures)
        );
    });

    assert!(allocations < 32, "{allocations} allocations");
}
