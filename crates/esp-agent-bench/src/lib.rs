//! Shared inputs for the host benchmarks of `esp-agent`.
//!
//! The benchmarks run on a regular computer with the potion-2M model of the example
//! firmware. Their numbers do not match the chip, but they show whether a change makes
//! the library faster or slower.

pub mod report;

use std::time::Duration;

use criterion::Criterion;
use esp_agent::{Model, model};

/// The potion-2M model of the example firmware.
pub static POTION: Model = model!("../../examples/semantic-search/model");

/// A short sentence, the typical query.
pub const SENTENCE: &str = "The quick brown fox jumps over the lazy dog.";

/// A Latin sentence with accented letters, which normalization has to decompose.
pub const ACCENTED: &str = "Où est le café? L'élève a reçu un prix à Noël.";

/// A Chinese sentence, which normalization pads with spaces around every character.
pub const CJK: &str = "今天天气很好，我们一起去公园散步吧，然后吃晚饭。";

/// Long made-up words that end in a rune the vocabulary lacks.
///
/// The tokenizer matches every letter of a word piece by piece and then drops the whole
/// word, which is the slowest path of the greedy longest match.
pub const UNKNOWN_WORDS: &str = "xqzvbkxqzvbkxqzvbkxqzvbkxqzvbkxqzvbkxqzvbkᚠ \
    qzxvkbqzxvkbqzxvkbqzxvkbqzxvkbqzxvkbqzxvkbqzxvkbᚠ \
    zvqxbkzvqxbkzvqxbkzvqxbkzvqxbkzvqxbkzvqxbkzvqxbkzvqxbkᚠ \
    kbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzᚠ \
    vzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqvzkbxqᚠ \
    bxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvbxqkzvᚠ \
    xkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvxkzqbvᚠ \
    qvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkqvbzxkᚠ \
    zbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxzbkvqxᚠ \
    vxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkvxqbzkᚠ";

/// The sentences of the example firmware, copied from its console.
pub const CORPUS_SENTENCES: [&str; 8] = [
    "The quick brown fox jumps over the lazy dog.",
    "A fast auburn fox leaps above a sleepy hound.",
    "Quarterly revenue grew 12% year over year.",
    "Qdrant is a vector search engine written in Rust.",
    "The ESP32-S3 is a dual-core microcontroller with Wi-Fi and Bluetooth.",
    "Paris is the capital of France and sits on the river Seine.",
    "Water boils at 100 degrees Celsius at sea level.",
    "Rust guarantees memory safety without a garbage collector.",
];

/// A long text of more than 512 potion tokens, the default truncation length.
#[must_use]
pub fn paragraph() -> String {
    CORPUS_SENTENCES.join(" ").repeat(8)
}

/// `len` distinct texts built from the sentences of the example firmware.
///
/// Text `i` is sentence `i % 8` followed by ` #i`, so the texts are deterministic.
#[must_use]
pub fn corpus(len: usize) -> Vec<String> {
    (0..len)
        .map(|i| {
            let sentence = CORPUS_SENTENCES[i % CORPUS_SENTENCES.len()];
            format!("{sentence} #{i}")
        })
        .collect()
}

/// Short measurement times that keep the whole run within the CI time budget.
#[must_use]
pub fn config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_millis(1500))
        .sample_size(50)
}
