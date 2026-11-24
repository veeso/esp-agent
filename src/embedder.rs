//! Static (Model2Vec-style) text embeddings: tokenize, look up rows, mean-pool.

use alloc::vec;
use alloc::vec::Vec;

use crate::WordPieceTokenizer;

/// Default truncation, in tokens (same default as the `statembed` crate).
pub const DEFAULT_MAX_LENGTH: usize = 512;

/// Smallest norm used as a divisor, so that zero vectors never divide by zero.
const NORM_EPSILON: f32 = 1e-12;

/// Errors raised when building a [`StaticEmbedder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedderError {
    /// The table byte length does not equal `vocab_size * dim * 4`.
    TableSizeMismatch {
        /// The byte length the table must have for the given vocabulary and dimension.
        expected: usize,
        /// The byte length of the table that was provided.
        found: usize,
    },
}

impl core::fmt::Display for EmbedderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TableSizeMismatch { expected, found } => {
                write!(
                    f,
                    "embedding table size mismatch: expected {expected} bytes, found {found} bytes"
                )
            }
        }
    }
}

impl core::error::Error for EmbedderError {}

/// Embeds text by averaging the rows of a static embedding table.
///
/// The text is split into WordPiece tokens, the row of every token is looked up in the
/// table, and the rows are mean-pooled into one vector of [`dim`](Self::dim) floats.
///
/// The embedding table is read directly from a `&'static [u8]` holding little-endian
/// `f32` rows. On the ESP32-S3 that slice is `.rodata`, which lives in flash and is
/// accessed through the cache/MMU like ordinary memory. The 7.2 MiB table is therefore
/// never copied into RAM, and each lookup touches only the rows of the tokens in the text.
pub struct StaticEmbedder {
    tokenizer: WordPieceTokenizer,
    table: &'static [u8],
    vocab_size: usize,
    dim: usize,
    /// Divides the pooled vector by its L2 norm, using a minimum divisor of `1e-12`.
    pub normalize: bool,
}

impl core::fmt::Debug for StaticEmbedder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StaticEmbedder")
            .field("tokenizer", &self.tokenizer)
            .field("vocab_size", &self.vocab_size)
            .field("dim", &self.dim)
            .field("normalize", &self.normalize)
            .finish_non_exhaustive()
    }
}

impl StaticEmbedder {
    /// Creates an embedder over a row-major table of little-endian `f32` rows.
    ///
    /// `table` is a `[vocab_size, dim]` matrix. Nothing is copied: the embedder reads the
    /// rows in place. [`normalize`](Self::normalize) is enabled by default.
    /// The byte size `vocab_size * dim * 4` must fit in a `usize`.
    ///
    /// # Errors
    ///
    /// Returns [`EmbedderError::TableSizeMismatch`] when `table.len()` is not
    /// `vocab_size * dim * 4`.
    ///
    /// # Panics
    ///
    /// Panics if the byte size overflows and integer overflow checks are enabled.
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

    /// Returns the number of floats in every embedding.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Returns the tokenizer that splits text into the ids looked up in the table.
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
    ///
    /// Text with no known tokens gives a zero vector.
    ///
    /// # Panics
    ///
    /// Panics if `out.len()` is not equal to [`dim`](Self::dim).
    pub fn embed_into(&self, text: &str, out: &mut [f32]) {
        assert_eq!(out.len(), self.dim, "output buffer must hold `dim` floats");
        self.pool(&self.tokenizer.encode(text), out);
    }

    /// Mean-pools the rows of `ids` into `out`, then optionally L2-normalizes.
    ///
    /// Supply an output buffer of [`dim`](Self::dim) floats for the full embedding.
    /// Every id counts toward the mean, including out-of-table ids, which contribute
    /// a zero row. When [`normalize`](Self::normalize) is enabled, a pooled vector
    /// with L2 norm below `1e-12` has length below one after normalization.
    pub fn pool(&self, ids: &[u32], out: &mut [f32]) {
        out.fill(0.0);
        if ids.is_empty() {
            return;
        }
        let row_bytes = self.dim * size_of::<f32>();
        // Ids outside the table cannot happen with a vocab built against it: skip them.
        let rows = ids.iter().filter_map(|&id| {
            let id = id as usize;
            (id < self.vocab_size).then(|| &self.table[id * row_bytes..(id + 1) * row_bytes])
        });
        for row in rows {
            for (acc, bytes) in out.iter_mut().zip(row.chunks_exact(4)) {
                *acc += f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            }
        }
        let n = ids.len() as f32;
        for x in out.iter_mut() {
            *x /= n;
        }
        if self.normalize {
            let norm = libm::sqrtf(out.iter().map(|v| v * v).sum::<f32>()).max(NORM_EPSILON);
            for x in out.iter_mut() {
                *x /= norm;
            }
        }
    }
}

/// Returns the cosine similarity of two equal-length vectors.
///
/// The dot product is divided by the product of the L2 norms, with a minimum
/// divisor of `1e-12`. For finite inputs, the result is `0.0` when either vector
/// is all zeros.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = libm::sqrtf(a.iter().map(|x| x * x).sum());
    let nb = libm::sqrtf(b.iter().map(|x| x * x).sum());
    dot / (na * nb).max(NORM_EPSILON)
}

#[cfg(test)]
mod tests {
    use super::{EmbedderError, StaticEmbedder, cosine_similarity};
    use crate::WordPieceTokenizer;
    use crate::test_util::{table, tokenizer, vocab};

    const ZERO: &[f32] = &[0.0, 0.0];

    /// An embedder over a 10-row, 2-dimensional table.
    ///
    /// Row 1 (`un`) is `[3, 4]`, row 4 (`hello`) is `[1, 0]`, row 6 (`world`) is
    /// `[0, 1]`, and every other row is zero.
    fn embedder() -> StaticEmbedder {
        let rows: [&[f32]; 10] = [
            ZERO,
            &[3.0, 4.0],
            ZERO,
            ZERO,
            &[1.0, 0.0],
            ZERO,
            &[0.0, 1.0],
            ZERO,
            ZERO,
            ZERO,
        ];
        StaticEmbedder::new(tokenizer(100, 512), table(&rows), 10, 2)
            .expect("the table matches the vocabulary size and dimension")
    }

    #[test]
    fn debug_summarizes_the_vocab_and_omits_the_table() {
        let tokenizer = WordPieceTokenizer::new(vocab(&[("abc", 0)]), 0, 100, 512);
        let embedder = StaticEmbedder::new(tokenizer, &[149, 193, 211, 239], 1, 1).unwrap();
        let debug = alloc::format!("{embedder:?}");
        assert!(debug.contains("blob_bytes: 16"));
        assert!(!debug.contains("97, 98, 99"));
        assert!(!debug.contains("149, 193, 211, 239"));
        assert!(debug.contains("vocab_size: 1"));
        assert!(debug.contains("dim: 1"));
        assert!(debug.contains("normalize: true"));
    }

    #[test]
    fn embed_mean_pools_rows() {
        let mut embedder = embedder();
        embedder.normalize = false;
        assert_eq!(embedder.embed("hello world"), [0.5, 0.5]);
    }

    #[test]
    fn embed_l2_normalizes_by_default() {
        let embedder = embedder();
        assert!(embedder.normalize);
        let vector = embedder.embed("un");
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn embed_into_clears_the_buffer_for_unknown_text() {
        let mut out = [9.0; 2];
        embedder().embed_into("xyz", &mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn zero_mean_vector_stays_zero_when_normalized() {
        let vector = embedder().embed(",");
        assert_eq!(vector, [0.0, 0.0]);
    }

    #[test]
    fn embed_into_matches_embed() {
        let embedder = embedder();
        let mut out = [9.0; 2];
        embedder.embed_into("hello world", &mut out);
        assert_eq!(out.as_slice(), embedder.embed("hello world"));
    }

    #[test]
    #[should_panic(expected = "output buffer must hold `dim` floats")]
    fn embed_into_rejects_wrong_buffer_length() {
        embedder().embed_into("hello", &mut [0.0; 3]);
    }

    #[test]
    fn pool_skips_ids_outside_the_table() {
        let mut out = [0.0; 2];
        let mut embedder = embedder();
        embedder.normalize = false;
        embedder.pool(&[4, 10], &mut out);
        assert_eq!(out, [0.5, 0.0]);
        embedder.pool(&[10, 4], &mut out);
        assert_eq!(out, [0.5, 0.0]);
    }

    #[test]
    fn new_rejects_a_table_of_the_wrong_size() {
        let error = StaticEmbedder::new(tokenizer(100, 512), table(&[&[1.0, 2.0]]), 10, 2)
            .expect_err("one row cannot fill a ten-row table");
        assert_eq!(
            error,
            EmbedderError::TableSizeMismatch {
                expected: 80,
                found: 8
            }
        );
        let diagnostic: &dyn core::error::Error = &error;
        let message = alloc::format!("{diagnostic}");
        assert!(message.contains("expected 80 bytes"));
        assert!(message.contains("found 8 bytes"));
    }

    #[test]
    fn normalized_pool_uses_a_minimum_divisor_for_tiny_rows() {
        let embedder = StaticEmbedder::new(tokenizer(100, 512), table(&[&[1e-13]]), 1, 1).unwrap();
        let mut out = [0.0];
        embedder.pool(&[0], &mut out);
        assert!((out[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_uses_a_minimum_divisor_for_tiny_norm_products() {
        let similarity = cosine_similarity(&[1e-7], &[1e-7]);
        assert!((similarity - 0.01).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_of_parallel_orthogonal_and_opposite_vectors() {
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[2.0, 0.0]), 1.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 3.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]), -1.0);
    }

    #[test]
    fn cosine_similarity_with_a_zero_vector_is_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }
}
