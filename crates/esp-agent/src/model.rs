//! A WordPiece embedding model that lives in flash.

use crate::_private::RawModel;
use crate::tokenizer::{Vocab, WordPieceTokenizer};

/// A WordPiece tokenizer and its embedding table, read in place from flash.
///
/// Create one with [`model!`](crate::model!). A `Model` is a handful of words that point
/// at the embedded data, so it is `Copy` and can be shared by any number of embedders.
#[derive(Clone, Copy)]
pub struct Model {
    vocab: Vocab,
    table: &'static [u8],
    vocab_size: usize,
    dim: usize,
    unk_id: u32,
    max_chars_per_word: usize,
}

impl core::fmt::Debug for Model {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Model")
            .field("vocab", &self.vocab)
            .field("table_bytes", &self.table.len())
            .field("vocab_size", &self.vocab_size)
            .field("dim", &self.dim)
            .field("unk_id", &self.unk_id)
            .field("max_chars_per_word", &self.max_chars_per_word)
            .finish()
    }
}

impl Model {
    /// Returns the number of floats in every embedding.
    #[must_use]
    pub const fn dim(&self) -> usize {
        self.dim
    }

    /// Returns the number of tokens in the vocabulary, which is the number of table rows.
    #[must_use]
    pub const fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    /// Slices the table out of `raw.file`.
    ///
    /// # Panics
    ///
    /// Panics when the table does not hold `vocab_size * dim` `f32` values or when its
    /// byte range lies outside the file.
    pub(crate) const fn from_raw(raw: RawModel) -> Self {
        let (head, _) = raw.file.split_at(raw.tensor_end);
        let (_, table) = head.split_at(raw.tensor_start);
        assert!(
            table.len() == raw.vocab_size * raw.dim * size_of::<f32>(),
            "model table size does not match its shape"
        );
        Self {
            vocab: Vocab::new(raw.vocab),
            table,
            vocab_size: raw.vocab_size,
            dim: raw.dim,
            unk_id: raw.unk_id,
            max_chars_per_word: raw.max_chars_per_word,
        }
    }

    /// Returns `true` when `other` is the same embedded model as `self`.
    ///
    /// Two models are the same when their table and vocabulary point at the same bytes
    /// and their shapes are equal. Models compiled by separate [`model!`](crate::model!)
    /// calls differ even when they read the same folder.
    #[cfg_attr(not(test), expect(dead_code, reason = "the agent is the first caller"))]
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        core::ptr::eq(self.table, other.table)
            && core::ptr::eq(self.vocab.blob(), other.vocab.blob())
            && self.vocab_size == other.vocab_size
            && self.dim == other.dim
    }

    /// Returns the little-endian `f32` table, a `[vocab_size, dim]` row-major matrix.
    pub(crate) const fn table(&self) -> &'static [u8] {
        self.table
    }

    /// Returns a tokenizer over this model's vocabulary that truncates at `max_length` tokens.
    pub(crate) const fn tokenizer(&self, max_length: usize) -> WordPieceTokenizer {
        WordPieceTokenizer::new(self.vocab, self.unk_id, self.max_chars_per_word, max_length)
    }
}

#[cfg(test)]
mod tests {
    use crate::_private::{RawModel, model};
    use crate::test_util;

    const ROWS: &[&[f32]] = &[&[1.0, 0.0], &[0.0, 1.0]];

    #[test]
    fn same_as_is_true_for_the_same_model() {
        let first = test_util::model(ROWS);
        let copy = first;

        assert!(first.same_as(&copy));
    }

    #[test]
    fn same_as_is_false_for_models_over_different_data() {
        let first = test_util::model(ROWS);
        let second = test_util::model(ROWS);

        assert!(!first.same_as(&second));
    }

    #[test]
    #[should_panic(expected = "model table size does not match its shape")]
    fn from_raw_rejects_a_table_of_the_wrong_size() {
        let _model = model(RawModel {
            file: &[0; 8],
            tensor_start: 0,
            tensor_end: 8,
            vocab: &[0; 4],
            vocab_size: 10,
            dim: 2,
            unk_id: 0,
            max_chars_per_word: 100,
        });
    }
}
