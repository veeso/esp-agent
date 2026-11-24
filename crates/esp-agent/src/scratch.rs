//! Reusable heap storage for tokenizing, embedding, and searching.

use alloc::vec::Vec;

use crate::tokenizer::NormalizedText;

/// Buffers that [`WordPieceTokenizer::encode_with`](crate::WordPieceTokenizer::encode_with),
/// [`Embedder::embed_with`](crate::Embedder::embed_with), and
/// [`Retriever::search_with`](crate::Retriever::search_with) reuse between calls.
///
/// The one-call methods such as [`Embedder::embed`](crate::Embedder::embed) allocate fresh
/// buffers on every call. A `Scratch` retains heap capacity for normalized text, combining
/// marks, token IDs, and the query vector. Subsequent calls allocate only when a buffer
/// needs to grow.
///
/// A `Scratch` holds no state that matters between calls: every call clears what it uses
/// before it writes. One `Scratch` can serve several embedders and retrievers, including
/// ones with different dimensions.
#[derive(Debug, Default, Clone)]
pub struct Scratch {
    /// The normalized text of the call in progress.
    pub(crate) normalized: NormalizedText,
    /// The token ids of the call in progress.
    pub(crate) ids: Vec<u32>,
    /// The query vector of a retriever search.
    pub(crate) query: Vec<f32>,
}

impl Scratch {
    /// Creates empty buffers, which grow on first use.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            normalized: NormalizedText::new(),
            ids: Vec::new(),
            query: Vec::new(),
        }
    }
}
