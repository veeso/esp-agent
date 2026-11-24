//! Static (Model2Vec-style) text embeddings: tokenize, look up rows, mean-pool.

use alloc::vec;
use alloc::vec::Vec;

use crate::model::Model;
use crate::tokenizer::WordPieceTokenizer;

/// Smallest norm used as a divisor, so that zero vectors never divide by zero.
const NORM_EPSILON: f32 = 1e-12;

/// Settings that decide how an [`Embedder`] turns text into a vector.
///
/// Change them at any time through [`Embedder::config_mut`]; the next call uses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmbedderConfig {
    /// Truncation, in tokens, counting `[UNK]` tokens. Zero embeds everything to the zero
    /// vector.
    pub max_length: usize,
    /// Divides the pooled vector by its L2 norm, using a minimum divisor of `1e-12`.
    pub normalize: bool,
}

impl EmbedderConfig {
    const DEFAULT: Self = Self {
        max_length: Self::DEFAULT_MAX_LENGTH,
        normalize: true,
    };

    /// Default truncation, in tokens (same default as the `statembed` crate).
    pub const DEFAULT_MAX_LENGTH: usize = 512;
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Embeds text by averaging the rows of a static embedding table.
///
/// The text is split into WordPiece tokens, the row of every token is looked up in the
/// [`Model`]'s table, and the rows are mean-pooled into one vector of [`dim`](Self::dim)
/// floats.
///
/// The table is read in place from flash, so an embedder never copies it and each lookup
/// touches only the rows of the tokens in the text. An embedder is `Copy`: embedders that
/// share one [`Model`] keep their own [`EmbedderConfig`].
#[derive(Debug, Clone, Copy)]
pub struct Embedder {
    model: Model,
    config: EmbedderConfig,
}

impl Embedder {
    /// Creates an embedder over `model` with the default [`EmbedderConfig`].
    #[must_use]
    pub const fn new(model: Model) -> Self {
        Self::with_config(model, EmbedderConfig::DEFAULT)
    }

    /// Creates an embedder over `model` with `config`.
    #[must_use]
    pub const fn with_config(model: Model, config: EmbedderConfig) -> Self {
        Self { model, config }
    }

    /// Returns the model this embedder reads.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.model
    }

    /// Returns the current configuration.
    #[must_use]
    pub const fn config(&self) -> &EmbedderConfig {
        &self.config
    }

    /// Returns the configuration for changes that apply from the next call on.
    pub fn config_mut(&mut self) -> &mut EmbedderConfig {
        &mut self.config
    }

    /// Returns the number of floats in every embedding.
    #[must_use]
    pub const fn dim(&self) -> usize {
        self.model.dim()
    }

    /// Returns a tokenizer over the model's vocabulary that truncates at the configured
    /// [`max_length`](EmbedderConfig::max_length).
    #[must_use]
    pub fn tokenizer(&self) -> WordPieceTokenizer {
        self.model.tokenizer(self.config.max_length)
    }

    /// Embeds `text` into a new vector of [`dim`](Self::dim) floats.
    #[must_use]
    pub fn embed(&self, text: &str) -> Vec<f32> {
        let mut out = vec![0f32; self.dim()];
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
        assert_eq!(
            out.len(),
            self.dim(),
            "output buffer must hold `dim` floats"
        );
        self.pool(&self.tokenizer().encode(text), out);
    }

    /// Mean-pools the rows of `ids` into `out`, then optionally L2-normalizes.
    ///
    /// Supply an output buffer of [`dim`](Self::dim) floats for the full embedding.
    /// Every id counts toward the mean, including out-of-table ids, which contribute
    /// a zero row. When [`normalize`](EmbedderConfig::normalize) is enabled, a pooled
    /// vector with L2 norm below `1e-12` has length below one after normalization.
    pub fn pool(&self, ids: &[u32], out: &mut [f32]) {
        out.fill(0.0);
        if ids.is_empty() {
            return;
        }
        let table = self.model.table();
        let vocab_size = self.model.vocab_size();
        let row_bytes = self.dim() * size_of::<f32>();
        // Ids outside the table cannot happen with a vocab built against it: skip them.
        let rows = ids.iter().filter_map(|&id| {
            let id = id as usize;
            (id < vocab_size).then(|| &table[id * row_bytes..(id + 1) * row_bytes])
        });
        for row in rows.clone() {
            for (acc, bytes) in out.iter_mut().zip(row.as_chunks::<4>().0) {
                *acc += f32::from_le_bytes(*bytes);
            }
        }
        for (column, sum) in out.iter_mut().enumerate() {
            if sum.is_finite() {
                *sum /= ids.len() as f32;
            } else {
                // Retry just the overflowing column, keeping ordinary rows on the f32 fast path.
                let wide_sum: f64 = rows
                    .clone()
                    .map(|row| f64::from(f32::from_le_bytes(row.as_chunks::<4>().0[column])))
                    .sum();
                *sum = (wide_sum / ids.len() as f64) as f32;
            }
        }
        if self.config.normalize {
            // Scale first so that squaring a finite component cannot overflow.
            let scale = out.iter().map(|x| x.abs()).fold(NORM_EPSILON, f32::max);
            for x in out.iter_mut() {
                *x /= scale;
            }
            let norm =
                libm::sqrtf(out.iter().map(|v| v * v).sum::<f32>()).max(NORM_EPSILON / scale);
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
    use super::{Embedder, EmbedderConfig, cosine_similarity};
    use crate::test_util;

    const ZERO: &[f32] = &[0.0, 0.0];

    /// An embedder over a 10-row, 2-dimensional table.
    ///
    /// Row 1 (`un`) is `[3, 4]`, row 4 (`hello`) is `[1, 0]`, row 6 (`world`) is
    /// `[0, 1]`, and every other row is zero.
    fn embedder(normalize: bool) -> Embedder {
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
        Embedder::with_config(
            test_util::model(&rows),
            EmbedderConfig {
                normalize,
                ..EmbedderConfig::default()
            },
        )
    }

    #[test]
    fn debug_summarizes_the_vocab_and_omits_the_table() {
        let marker = f32::from_le_bytes([149, 193, 211, 239]);
        let embedder = Embedder::new(test_util::model(&[&[marker]]));
        let debug = alloc::format!("{embedder:?}");
        assert!(debug.contains("blob_bytes"));
        assert!(!debug.contains("149, 193, 211, 239"));
        assert!(debug.contains("table_bytes: 4"));
        assert!(debug.contains("vocab_size: 1"));
        assert!(debug.contains("dim: 1"));
        assert!(debug.contains("normalize: true"));
    }

    #[test]
    fn default_config_truncates_at_512_tokens_and_normalizes() {
        let config = EmbedderConfig::default();
        assert_eq!(config.max_length, 512);
        assert!(config.normalize);
    }

    #[test]
    fn max_length_change_applies_to_the_next_call() {
        let mut embedder = embedder(false);
        embedder.config_mut().max_length = 1;
        assert_eq!(embedder.tokenizer().max_length(), 1);
        assert_eq!(embedder.tokenizer().encode("hello world"), [4]);
        assert_eq!(embedder.embed("hello world"), [1.0, 0.0]);
        embedder.config_mut().max_length = 2;
        assert_eq!(embedder.tokenizer().encode("hello world"), [4, 6]);
        assert_eq!(embedder.embed("hello world"), [0.5, 0.5]);
    }

    #[test]
    fn normalize_can_be_switched_off_and_on_at_run_time() {
        let mut embedder = embedder(true);
        embedder.config_mut().normalize = false;
        assert_eq!(embedder.embed("un"), [3.0, 4.0]);
        embedder.config_mut().normalize = true;
        assert_eq!(embedder.embed("un"), [0.6, 0.8]);
    }

    #[test]
    fn embedders_sharing_a_model_keep_their_own_config() {
        let model = *embedder(true).model();
        let raw = Embedder::with_config(
            model,
            EmbedderConfig {
                normalize: false,
                ..EmbedderConfig::default()
            },
        );
        let unit = Embedder::new(model);

        assert_eq!(raw.embed("un"), [3.0, 4.0]);
        let vector = unit.embed("un");
        assert!((vector[0] - 0.6).abs() < 1e-6);
        assert!((vector[1] - 0.8).abs() < 1e-6);
        assert!(!raw.config().normalize);
        assert!(unit.config().normalize);
    }

    #[test]
    fn zero_max_length_embeds_to_zero() {
        let mut embedder = embedder(true);
        embedder.config_mut().max_length = 0;
        assert_eq!(embedder.embed("hello world"), [0.0, 0.0]);
    }

    #[test]
    fn embed_mean_pools_rows() {
        assert_eq!(embedder(false).embed("hello world"), [0.5, 0.5]);
    }

    #[test]
    fn embed_into_clears_the_buffer_for_unknown_text() {
        let mut out = [9.0; 2];
        embedder(true).embed_into("xyz", &mut out);
        assert_eq!(out, [0.0, 0.0]);
    }

    #[test]
    fn zero_mean_vector_stays_zero_when_normalized() {
        let vector = embedder(true).embed(",");
        assert_eq!(vector, [0.0, 0.0]);
    }

    #[test]
    fn embed_into_matches_embed() {
        let embedder = embedder(true);
        let mut out = [9.0; 2];
        embedder.embed_into("hello world", &mut out);
        assert_eq!(out.as_slice(), embedder.embed("hello world"));
    }

    #[test]
    #[should_panic(expected = "output buffer must hold `dim` floats")]
    fn embed_into_rejects_wrong_buffer_length() {
        embedder(true).embed_into("hello", &mut [0.0; 3]);
    }

    #[test]
    fn pool_skips_ids_outside_the_table() {
        let mut out = [0.0; 2];
        let embedder = embedder(false);
        embedder.pool(&[4, 10], &mut out);
        assert_eq!(out, [0.5, 0.0]);
        embedder.pool(&[10, 4], &mut out);
        assert_eq!(out, [0.5, 0.0]);
    }

    #[test]
    fn repeated_large_rows_have_a_finite_mean() {
        let mut embedder = Embedder::new(test_util::model(&[&[0.0], &[3e38]]));
        embedder.config_mut().normalize = false;
        assert_eq!(embedder.embed("un un"), [3e38]);
        embedder.config_mut().normalize = true;
        assert_eq!(embedder.embed("un un"), [1.0]);
    }

    #[test]
    fn normalized_pool_handles_large_rows() {
        let embedder = Embedder::new(test_util::model(&[&[1e20, 0.0, 0.0]]));
        let mut out = [0.0; 3];
        embedder.pool(&[0], &mut out);
        assert_eq!(out, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn normalized_pool_uses_a_minimum_divisor_for_tiny_rows() {
        let embedder = Embedder::new(test_util::model(&[&[1e-13]]));
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
