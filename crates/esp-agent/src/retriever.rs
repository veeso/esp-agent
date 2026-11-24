//! Search over stored texts: an [`Embedder`] paired with an [`Index`].

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::embedder::Embedder;
use crate::index::Index;

/// A stored text and its associated payload, found by [`Retriever::search`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextHit<'a, T> {
    /// Cosine similarity between the query and the stored text.
    pub score: f32,
    /// Insertion position of the entry, starting at zero.
    pub index: usize,
    /// The original text supplied on insertion.
    pub text: &'a str,
    /// The payload stored with the text.
    pub payload: &'a T,
}

/// Stores texts with a payload each and returns the ones closest in meaning to a query.
///
/// A retriever embeds every text it is given with its [`Embedder`], keeps the vector in
/// an [`Index`], and embeds each query the same way. Hits come back by descending cosine
/// similarity, and equal scores keep their insertion order. A query with no known words
/// scores every entry `0.0`, so the hits are the first entries in insertion order.
///
/// The embedder is read-only: changing its configuration after inserting texts would mix
/// vectors built with different settings.
#[derive(Debug, Clone)]
pub struct Retriever<T> {
    embedder: Embedder,
    index: Index<T>,
    texts: Vec<Box<str>>,
}

impl<T> Retriever<T> {
    /// Creates an empty retriever that embeds with `embedder`.
    #[must_use]
    pub const fn new(embedder: Embedder) -> Self {
        let index = Index::new(embedder.dim());
        Self {
            embedder,
            index,
            texts: Vec::new(),
        }
    }

    /// Creates an empty retriever that can hold `capacity` texts without reallocating.
    #[must_use]
    pub fn with_capacity(embedder: Embedder, capacity: usize) -> Self {
        let index = Index::with_capacity(embedder.dim(), capacity);
        Self {
            embedder,
            index,
            texts: Vec::with_capacity(capacity),
        }
    }

    /// Returns the embedder that turns texts and queries into vectors.
    #[must_use]
    pub const fn embedder(&self) -> &Embedder {
        &self.embedder
    }

    /// Returns the index that holds the vectors of the stored texts.
    #[must_use]
    pub const fn index(&self) -> &Index<T> {
        &self.index
    }

    /// Returns the number of stored texts.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns `true` when no text is stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Embeds `text` and stores it with its `payload`.
    pub fn insert(&mut self, text: &str, payload: T) {
        let vector = self.embedder.embed(text);
        self.index.insert(&vector, payload);
        self.texts.push(text.into());
    }

    /// Returns the `k` stored texts closest to `query`, best first.
    ///
    /// Fewer than `k` hits come back when fewer texts are stored, and none when `k` is
    /// zero.
    #[must_use]
    pub fn search(&self, query: &str, k: usize) -> Vec<TextHit<'_, T>> {
        if k == 0 || self.is_empty() {
            return Vec::new();
        }
        self.index
            .search_with(&self.embedder.embed(query), k, |hit| TextHit {
                score: hit.score,
                index: hit.index,
                text: &self.texts[hit.index],
                payload: hit.payload,
            })
    }
}

impl<T, S> Extend<(S, T)> for Retriever<T>
where
    S: AsRef<str>,
{
    /// Embeds and stores every `(text, payload)` pair.
    fn extend<I: IntoIterator<Item = (S, T)>>(&mut self, iter: I) {
        for (text, payload) in iter {
            self.insert(text.as_ref(), payload);
        }
    }
}
