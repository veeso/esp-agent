use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use esp_agent::{Agent, Embedder, Knowledge, Scratch, cosine_similarity};
use esp_agent_bench::{CORPUS_SENTENCES, POTION, SENTENCE, corpus};

/// Number of stored texts that the search benchmarks cover. 8 is the corpus of the example
/// firmware.
const SIZES: [usize; 3] = [8, 1024, 4096];

/// Number of hits that the example firmware asks for.
const TOP_K: usize = 3;

fn knowledge(embedder: Embedder, len: usize) -> Knowledge<usize> {
    let mut knowledge = Knowledge::with_capacity(embedder, len);
    knowledge.extend(corpus(len).into_iter().zip(0..));
    knowledge
}

fn search(c: &mut Criterion) {
    let embedder = Embedder::new(POTION);
    let a = embedder.embed(SENTENCE);
    let b = embedder.embed(CORPUS_SENTENCES[1]);
    let query = embedder.embed(SENTENCE);
    let knowledges: Vec<_> = SIZES.iter().map(|&len| knowledge(embedder, len)).collect();
    let agents: Vec<_> = knowledges
        .iter()
        .map(|knowledge| Agent::new(POTION, knowledge).expect("the corpus is embedded with POTION"))
        .collect();

    // The query is the first stored text without its suffix, so it must be among the hits.
    let hits = agents[0].prompt(SENTENCE, TOP_K);
    assert_eq!(hits.len(), TOP_K);
    assert!(hits.iter().any(|hit| hit.index == 0));

    let mut group = c.benchmark_group("search");
    group.bench_function("cosine_similarity/dim64", |bencher| {
        bencher.iter(|| cosine_similarity(black_box(&a), black_box(&b)));
    });
    for len in [8, 1024] {
        group.bench_function(BenchmarkId::new("insert", len), |bencher| {
            let texts = corpus(len);
            bencher.iter(|| {
                let mut knowledge = Knowledge::with_capacity(embedder, len);
                knowledge.extend(texts.iter().zip(0..));
                knowledge
            });
        });
    }
    for (agent, len) in agents.iter().zip(SIZES) {
        group.bench_function(BenchmarkId::new("prompt", len), |bencher| {
            bencher.iter(|| agent.prompt(black_box(SENTENCE), TOP_K));
        });
        group.bench_function(BenchmarkId::new("prompt_with", len), |bencher| {
            let mut scratch = Scratch::new();
            let mut hits = Vec::with_capacity(TOP_K);
            bencher.iter(|| {
                agent.prompt_with(black_box(SENTENCE), TOP_K, &mut scratch, &mut hits);
                hits.len()
            });
        });
        group.bench_function(BenchmarkId::new("index_search", len), |bencher| {
            bencher.iter(|| agent.knowledge().index().search(black_box(&query), TOP_K));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = esp_agent_bench::config();
    targets = search
}
criterion_main!(benches);
