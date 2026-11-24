//! Static (Model2Vec-style) text embeddings: tokenize, look up rows, mean-pool.
//!
//! The embedding table is read directly from a `&'static [u8]` holding little-endian
//! `f32` rows. On the ESP32-S3 that slice is `.rodata`, which lives in flash and is
//! accessed through the cache/MMU like ordinary memory. The 7.2 MiB table is therefore
//! never copied into RAM, and each lookup touches only the rows of the tokens in the text.

use alloc::vec;
use alloc::vec::Vec;

use crate::tokenizer::WordPieceTokenizer;

/// Default truncation, in tokens (same default as the `statembed` crate).
pub const DEFAULT_MAX_LENGTH: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedderError {
    /// The table byte length does not equal `vocab_size * dim * 4`.
    TableSizeMismatch { expected: usize, found: usize },
}

pub struct StaticEmbedder {
    tokenizer: WordPieceTokenizer,
    table: &'static [u8],
    vocab_size: usize,
    dim: usize,
    /// L2-normalize the output (potion models are meant to be used normalized).
    pub normalize: bool,
}

impl StaticEmbedder {
    /// `table` is a row-major `[vocab_size, dim]` matrix of little-endian `f32`.
    pub fn new(
        tokenizer: WordPieceTokenizer,
        table: &'static [u8],
        vocab_size: usize,
        dim: usize,
    ) -> Result<Self, EmbedderError> {
        let expected = vocab_size * dim * size_of::<f32>();
        if table.len() != expected {
            return Err(EmbedderError::TableSizeMismatch {
                expected,
                found: table.len(),
            });
        }
        Ok(Self {
            tokenizer,
            table,
            vocab_size,
            dim,
            normalize: true,
        })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn tokenizer(&self) -> &WordPieceTokenizer {
        &self.tokenizer
    }

    /// Embeds `text` into a new vector of [`dim`](Self::dim) floats.
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut out = vec![0f32; self.dim];
        self.embed_into(text, &mut out);
        out
    }

    /// Embeds `text` into `out`, which must hold exactly [`dim`](Self::dim) floats.
    /// Text with no known tokens gives a zero vector.
    pub fn embed_into(&self, text: &str, out: &mut [f32]) {
        assert_eq!(out.len(), self.dim, "output buffer must hold `dim` floats");
        self.pool(&self.tokenizer.encode(text), out);
    }

    /// Mean-pools the rows of `ids` into `out`, then optionally L2-normalizes.
    pub fn pool(&self, ids: &[u32], out: &mut [f32]) {
        out.fill(0.0);
        if ids.is_empty() {
            return;
        }
        let row_bytes = self.dim * size_of::<f32>();
        for &id in ids {
            let id = id as usize;
            if id >= self.vocab_size {
                continue; // cannot happen with a vocab built against this table
            }
            let row = &self.table[id * row_bytes..(id + 1) * row_bytes];
            for (acc, bytes) in out.iter_mut().zip(row.chunks_exact(4)) {
                *acc += f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
        }
        let n = ids.len() as f32;
        for x in out.iter_mut() {
            *x /= n;
        }
        if self.normalize {
            let norm = libm::sqrtf(out.iter().map(|v| v * v).sum::<f32>()).max(1e-12);
            for x in out.iter_mut() {
                *x /= norm;
            }
        }
    }
}

/// Cosine similarity of two equal-length vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = libm::sqrtf(a.iter().map(|x| x * x).sum());
    let nb = libm::sqrtf(b.iter().map(|x| x * x).sum());
    dot / (na * nb).max(1e-12)
}
