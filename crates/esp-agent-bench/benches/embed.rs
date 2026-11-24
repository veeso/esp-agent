use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use esp_agent::Embedder;
use esp_agent_bench::{POTION, SENTENCE, paragraph};

fn embed(c: &mut Criterion) {
    let embedder = Embedder::new(POTION);
    let paragraph = paragraph();
    let ids = embedder.tokenizer().encode(&paragraph);
    assert_eq!(ids.len(), 512);
    let mut out = vec![0.0; embedder.dim()];

    let mut group = c.benchmark_group("embed");
    group.bench_function("embed/sentence", |b| {
        b.iter(|| embedder.embed(black_box(SENTENCE)));
    });
    group.bench_function("embed/paragraph", |b| {
        b.iter(|| embedder.embed(black_box(&paragraph)));
    });
    group.bench_function("embed_into/sentence", |b| {
        b.iter(|| {
            embedder.embed_into(black_box(SENTENCE), &mut out);
            out[0]
        });
    });
    group.bench_function("pool/512_ids", |b| {
        b.iter(|| {
            embedder.pool(black_box(&ids), &mut out);
            out[0]
        });
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = esp_agent_bench::config();
    targets = embed
}
criterion_main!(benches);
