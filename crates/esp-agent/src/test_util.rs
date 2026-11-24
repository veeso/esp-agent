//! Builders for tiny vocabularies and tables used by unit tests.

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::_private::RawModel;
use crate::model::Model;
use crate::tokenizer::{Vocab, WordPieceTokenizer};

/// Tiny vocabulary shared by the tokenizer and embedder tests.
///
/// The `é` entry never matches: normalization strips the accent before lookup.
pub(crate) const VOCAB: &[(&str, u32)] = &[
    ("[UNK]", 0),
    ("un", 1),
    ("##aff", 2),
    ("##able", 3),
    ("hello", 4),
    (",", 5),
    ("world", 6),
    ("é", 7),
    ("中", 8),
    ("文", 9),
];

/// Encodes `entries` in the vocabulary blob layout and leaks the buffer.
pub(crate) fn vocab_blob(entries: &[(&str, u32)]) -> &'static [u8] {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(token, _)| token.as_bytes());

    let header = 4 + 4 * sorted.len();
    let mut offsets = Vec::new();
    let mut body = Vec::new();
    for (token, id) in &sorted {
        offsets.push(u32::try_from(header + body.len()).expect("test vocab fits in u32"));
        body.extend_from_slice(&id.to_le_bytes());
        body.push(u8::try_from(token.len()).expect("test token fits in u8"));
        body.extend_from_slice(token.as_bytes());
    }

    let mut blob = Vec::new();
    blob.extend_from_slice(
        &u32::try_from(sorted.len())
            .expect("test vocab fits in u32")
            .to_le_bytes(),
    );
    for offset in offsets {
        blob.extend_from_slice(&offset.to_le_bytes());
    }
    blob.extend_from_slice(&body);
    Box::leak(blob.into_boxed_slice())
}

/// A vocabulary over `entries`.
pub(crate) fn vocab(entries: &[(&str, u32)]) -> Vocab {
    Vocab::new(vocab_blob(entries))
}

/// A tokenizer over [`VOCAB`] whose unknown id is 0.
pub(crate) fn tokenizer(max_chars_per_word: usize, max_length: usize) -> WordPieceTokenizer {
    WordPieceTokenizer::new(vocab(VOCAB), 0, max_chars_per_word, max_length)
}

/// Concatenates `rows` as little-endian `f32` and leaks the buffer.
pub(crate) fn table(rows: &[&[f32]]) -> &'static [u8] {
    let bytes: Vec<u8> = rows
        .iter()
        .flat_map(|row| row.iter())
        .flat_map(|value| value.to_le_bytes())
        .collect();
    Box::leak(bytes.into_boxed_slice())
}

/// A model over [`VOCAB`] whose table is `rows` and whose unknown id is 0.
pub(crate) fn model(rows: &[&[f32]]) -> Model {
    let table = table(rows);
    crate::_private::model(RawModel {
        file: table,
        tensor_start: 0,
        tensor_end: table.len(),
        vocab: vocab_blob(VOCAB),
        vocab_size: rows.len(),
        dim: rows.first().map_or(0, |row| row.len()),
        unk_id: 0,
        max_chars_per_word: 100,
    })
}
