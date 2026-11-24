//! The agent firmware queries with prompts.

use alloc::vec::Vec;
use core::fmt;

use crate::index::Hit;
use crate::knowledge::Knowledge;
use crate::model::Model;
use crate::retriever::TextHit;
use crate::scratch::Scratch;

/// The reason an [`Agent`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AgentError {
    /// The knowledge store was embedded with a different model than the agent's.
    ///
    /// Vectors of two models are not comparable, so the agent refuses the store instead of
    /// returning meaningless scores.
    ModelMismatch,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelMismatch => {
                f.write_str("the knowledge was embedded with a different model than the agent's")
            }
        }
    }
}

impl core::error::Error for AgentError {}

/// An agent that answers prompts with the texts of a [`Knowledge`] store closest in meaning.
///
/// An agent holds its [`Model`] and borrows a store, so any number of agents can share
/// one store without copying it. Build it with [`Agent::new`], which checks that the store
/// was embedded with the same model, then ask it with [`prompt`](Self::prompt). Hits come
/// back by descending cosine similarity, and equal scores keep their insertion order. A
/// prompt with no known words scores every entry `0.0`, so the hits are the first entries
/// in insertion order.
///
/// Hits borrow the store, not the agent, so they stay valid after the agent is dropped.
pub struct Agent<'k, T> {
    model: Model,
    knowledge: &'k Knowledge<T>,
}

impl<T: fmt::Debug> fmt::Debug for Agent<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agent")
            .field("model", &self.model)
            .field("knowledge", &self.knowledge)
            .finish()
    }
}

impl<T> Clone for Agent<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Agent<'_, T> {}

impl<'k, T> Agent<'k, T> {
    /// Creates an agent that answers prompts from `knowledge`.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::ModelMismatch`] when `knowledge` was not embedded with
    /// `model`, even if it is empty. Two models are the same when they come from the
    /// same [`model!`](crate::model!) call.
    pub fn new(model: Model, knowledge: &'k Knowledge<T>) -> Result<Self, AgentError> {
        if model.same_as(knowledge.embedder().model()) {
            Ok(Self { model, knowledge })
        } else {
            Err(AgentError::ModelMismatch)
        }
    }

    /// Returns the `k` stored texts closest to `prompt`, best first.
    ///
    /// The prompt is embedded with the store's embedder, so it uses the settings that built
    /// the stored vectors. Fewer than `k` hits come back when fewer texts are stored, and
    /// none when `k` is zero.
    #[must_use]
    pub fn prompt(&self, prompt: &str, k: usize) -> Vec<TextHit<'k, T>> {
        self.knowledge.retriever().search(prompt, k)
    }

    /// Replaces the contents of `hits` with the `k` stored texts closest to `prompt`, best
    /// first.
    ///
    /// Like [`prompt`](Self::prompt), but embeds the prompt in the buffers of `scratch` and
    /// fills a vector you keep. With a warm `scratch` and a `hits` that has room for `k`
    /// hits, the call is allocation-free. One `scratch` can serve agents of different
    /// dimensions. Pass a hit's [`index`](Hit::index) to [`Knowledge::text`] to borrow the
    /// original text.
    pub fn prompt_with(
        &self,
        prompt: &str,
        k: usize,
        scratch: &mut Scratch,
        hits: &mut Vec<Hit<'k, T>>,
    ) {
        self.knowledge
            .retriever()
            .search_with(prompt, k, scratch, hits);
    }

    /// Returns the model the agent was built with.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.model
    }

    /// Returns the store the agent answers from.
    #[must_use]
    pub const fn knowledge(&self) -> &'k Knowledge<T> {
        self.knowledge
    }
}
