use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use esp_agent::{Embedder, EmbedderConfig, normalize};
use esp_agent_bench::{ACCENTED, CJK, POTION, SENTENCE, UNKNOWN_WORDS, paragraph};

fn tokenize(c: &mut Criterion) {
    let tokenizer = Embedder::new(POTION).tokenizer();
    let paragraph = paragraph();

    // Check that the inputs exercise what the benchmark names promise.
    assert_eq!(
        tokenizer.encode(&paragraph).len(),
        EmbedderConfig::DEFAULT_MAX_LENGTH
    );
    assert!(tokenizer.encode(UNKNOWN_WORDS).is_empty());
    assert_ne!(normalize(ACCENTED), ACCENTED.to_lowercase());

    let mut group = c.benchmark_group("tokenize");
    group.bench_function("normalize/ascii", |b| {
        b.iter(|| normalize(black_box(SENTENCE)));
    });
    group.bench_function("normalize/accented", |b| {
        b.iter(|| normalize(black_box(ACCENTED)));
    });
    group.bench_function("normalize/cjk", |b| {
        b.iter(|| normalize(black_box(CJK)));
    });
    group.bench_function("encode/sentence", |b| {
        b.iter(|| tokenizer.encode(black_box(SENTENCE)));
    });
    group.bench_function("encode/paragraph", |b| {
        b.iter(|| tokenizer.encode(black_box(&paragraph)));
    });
    group.bench_function("encode/unknown_words", |b| {
        b.iter(|| tokenizer.encode(black_box(UNKNOWN_WORDS)));
    });
    group.bench_function("encode_with_unk/sentence", |b| {
        b.iter(|| tokenizer.encode_with_unk(black_box(SENTENCE)));
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = esp_agent_bench::config();
    targets = tokenize
}
criterion_main!(benches);
