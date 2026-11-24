//! The store of texts that agents borrow.

use crate::embedder::Embedder;
use crate::index::Index;
use crate::retriever::Retriever;

/// Stores texts with a payload each, to be shared by any number of agents.
///
/// A knowledge store embeds every text it is given with its [`Embedder`] and keeps the
/// vector in an [`Index`] next to the original text. Fill it once, then lend it to an
/// [`Agent`](crate::Agent), which answers prompts with the stored texts closest in
/// meaning.
///
/// The embedder is read-only: changing its configuration after inserting texts would mix
/// vectors built with different settings.
#[derive(Debug, Clone)]
pub struct Knowledge<T> {
    retriever: Retriever<T>,
}

impl<T> Knowledge<T> {
    /// Creates an empty store that embeds with `embedder`.
    #[must_use]
    pub const fn new(embedder: Embedder) -> Self {
        Self {
            retriever: Retriever::new(embedder),
        }
    }

    /// Creates an empty store that can hold `capacity` texts without reallocating.
    #[must_use]
    pub fn with_capacity(embedder: Embedder, capacity: usize) -> Self {
        Self {
            retriever: Retriever::with_capacity(embedder, capacity),
        }
    }

    /// Returns the embedder that turns texts and prompts into vectors.
    #[must_use]
    pub const fn embedder(&self) -> &Embedder {
        self.retriever.embedder()
    }

    /// Returns the index that holds the vectors of the stored texts.
    #[must_use]
    pub const fn index(&self) -> &Index<T> {
        self.retriever.index()
    }

    /// Returns the original text at an insertion index, or `None` if it is out of bounds.
    #[must_use]
    pub fn text(&self, index: usize) -> Option<&str> {
        self.retriever.text(index)
    }

    /// Returns the number of stored texts.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.retriever.len()
    }

    /// Returns `true` when no text is stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.retriever.is_empty()
    }

    /// Embeds `text` and stores it with its `payload`.
    pub fn insert(&mut self, text: &str, payload: T) {
        self.retriever.insert(text, payload);
    }

    /// Returns the search over the stored texts.
    pub(crate) const fn retriever(&self) -> &Retriever<T> {
        &self.retriever
    }
}

impl<T, S> Extend<(S, T)> for Knowledge<T>
where
    S: AsRef<str>,
{
    /// Embeds and stores every `(text, payload)` pair.
    fn extend<I: IntoIterator<Item = (S, T)>>(&mut self, iter: I) {
        self.retriever.extend(iter);
    }
}
