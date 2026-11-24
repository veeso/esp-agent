//! Search over stored texts: an [`Embedder`] paired with an [`Index`].

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::embedder::Embedder;
use crate::index::{Hit, Index};
use crate::scratch::Scratch;

/// A stored text and its associated payload, returned by [`Agent::prompt`](crate::Agent::prompt).
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
pub(crate) struct Retriever<T> {
    embedder: Embedder,
    index: Index<T>,
    texts: Vec<Box<str>>,
}

impl<T> Retriever<T> {
    /// Creates an empty retriever that embeds with `embedder`.
    #[must_use]
    pub(crate) const fn new(embedder: Embedder) -> Self {
        let index = Index::new(embedder.dim());
        Self {
            embedder,
            index,
            texts: Vec::new(),
        }
    }

    /// Creates an empty retriever that can hold `capacity` texts without reallocating.
    #[must_use]
    pub(crate) fn with_capacity(embedder: Embedder, capacity: usize) -> Self {
        let index = Index::with_capacity(embedder.dim(), capacity);
        Self {
            embedder,
            index,
            texts: Vec::with_capacity(capacity),
        }
    }

    /// Returns the embedder that turns texts and queries into vectors.
    #[must_use]
    pub(crate) const fn embedder(&self) -> &Embedder {
        &self.embedder
    }

    /// Returns the index that holds the vectors of the stored texts.
    #[must_use]
    pub(crate) const fn index(&self) -> &Index<T> {
        &self.index
    }

    /// Returns the original text at an insertion index, or `None` if it is out of bounds.
    #[must_use]
    pub(crate) fn text(&self, index: usize) -> Option<&str> {
        self.texts.get(index).map(AsRef::as_ref)
    }

    /// Returns the number of stored texts.
    #[must_use]
    pub(crate) const fn len(&self) -> usize {
        self.index.len()
    }

    /// Returns `true` when no text is stored.
    #[must_use]
    pub(crate) const fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Embeds `text` and stores it with its `payload`.
    pub(crate) fn insert(&mut self, text: &str, payload: T) {
        let vector = self.embedder.embed(text);
        self.index.insert(&vector, payload);
        self.texts.push(text.into());
    }

    /// Returns the `k` stored texts closest to `query`, best first.
    ///
    /// Fewer than `k` hits come back when fewer texts are stored, and none when `k` is
    /// zero.
    #[must_use]
    pub(crate) fn search(&self, query: &str, k: usize) -> Vec<TextHit<'_, T>> {
        let mut hits = Vec::with_capacity(k.min(self.len()));
        self.search_with(query, k, &mut Scratch::new(), &mut hits);
        hits.into_iter()
            .map(|hit| TextHit {
                score: hit.score,
                index: hit.index,
                text: &self.texts[hit.index],
                payload: hit.payload,
            })
            .collect()
    }

    /// Replaces the contents of `hits` with the `k` stored texts closest to `query`, best
    /// first.
    ///
    /// Like [`search`](Self::search), but embeds the query in the buffers of `scratch` and
    /// fills a vector you keep. With a warm `scratch` and a `hits` that has room for `k`
    /// hits, the call is allocation-free. One `scratch` can serve retrievers of different
    /// dimensions. Pass a hit's [`index`](Hit::index) to [`text`](Self::text) to borrow
    /// the original text.
    pub(crate) fn search_with<'a>(
        &'a self,
        query: &str,
        k: usize,
        scratch: &mut Scratch,
        hits: &mut Vec<Hit<'a, T>>,
    ) {
        if k == 0 || self.is_empty() {
            hits.clear();
            return;
        }
        let Scratch {
            normalized,
            ids,
            query: vector,
        } = scratch;
        vector.resize(self.embedder.dim(), 0.0);
        self.embedder.embed_parts(query, normalized, ids, vector);
        self.index.search_into(vector, k, hits);
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
