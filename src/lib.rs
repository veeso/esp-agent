//! Bare-metal building blocks for a local agent on ESP32 chips.
//!
//! The crate currently runs the `potion-2M` static embedding model on device. It is
//! based on [`static-models-bm`](https://github.com/qdrant-labs/static-models-bm) by
//! [AstraBert](https://github.com/AstraBert).
//!
//! Everything the model needs is embedded in the firmware image:
//!
//! * `model/model.safetensors` is included as-is and stays in flash (read in place).
//! * `model/tokenizer.json` is compiled by `build.rs` into a sorted binary vocabulary,
//!   also read in place.
//!
//! The only heap use at run time is small per-request scratch (normalized text, token
//! ids, the output vector).
//!
//! ## Feature flags
//!
//! Select exactly one chip feature. ESP32-S31 builds require
//! `--no-default-features --features esp32s31`.
//!
//! | name       | description          | default |
//! |------------|----------------------|---------|
//! | `esp32s3`  | Build for ESP32-S3.  | ✔       |
//! | `esp32s31` | Build for ESP32-S31. |         |
#![no_std]

extern crate alloc;

pub mod console;
pub mod embedder;
pub mod tokenizer;

pub use embedder::{DEFAULT_MAX_LENGTH, EmbedderError, StaticEmbedder, cosine_similarity};
pub use tokenizer::{Vocab, WordPieceTokenizer};

mod meta {
    include!(concat!(env!("OUT_DIR"), "/model_meta.rs"));
}

/// Number of tokens in the vocabulary (rows of the embedding table).
pub const VOCAB_SIZE: usize = meta::VOCAB_SIZE;
/// Embedding dimension.
pub const DIM: usize = meta::DIM;

/// Forces 4-byte alignment of the embedded bytes.
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

static MODEL_FILE: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/model/model.safetensors"
)));

static VOCAB_BLOB: &Aligned<[u8]> =
    &Aligned(*include_bytes!(concat!(env!("OUT_DIR"), "/vocab.bin")));

/// The raw `f32` embedding table, a `[VOCAB_SIZE, DIM]` row-major matrix, in flash.
pub fn embedding_table() -> &'static [u8] {
    &MODEL_FILE.0[meta::TENSOR_START..meta::TENSOR_END]
}

/// The tokenizer for the embedded model.
pub fn tokenizer() -> WordPieceTokenizer {
    WordPieceTokenizer::new(
        Vocab::new(&VOCAB_BLOB.0),
        meta::UNK_ID,
        meta::MAX_CHARS_PER_WORD,
        DEFAULT_MAX_LENGTH,
    )
}

/// The embedder for the model embedded in the firmware. Cheap: nothing is copied.
pub fn embedder() -> StaticEmbedder {
    StaticEmbedder::new(tokenizer(), embedding_table(), VOCAB_SIZE, DIM)
        .expect("build.rs validated the table size")
}
