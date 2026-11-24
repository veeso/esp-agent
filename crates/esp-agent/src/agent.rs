//! The agent firmware queries with prompts.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use crate::context::{
    Command, Context, Pending, TargetId, TargetMatch, TargetSelector, TurnHandler, TurnOutcome,
};
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
///
/// An agent also owns the memory of its conversation, which [`Agent::builder`] enables and
/// [`turn`](Self::turn) uses. The memory makes an agent cloneable only when its payload is,
/// and never `Copy`: a clone has its own independent conversation.
pub struct Agent<'k, T> {
    model: Model,
    knowledge: &'k Knowledge<T>,
    context: Context<T>,
}

impl<T: fmt::Debug> fmt::Debug for Agent<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Agent")
            .field("model", &self.model)
            .field("knowledge", &self.knowledge)
            .field("context", &self.context)
            .finish()
    }
}

impl<T: Clone> Clone for Agent<'_, T> {
    fn clone(&self) -> Self {
        Self {
            model: self.model,
            knowledge: self.knowledge,
            context: self.context.clone(),
        }
    }
}

/// Configures and builds an [`Agent`].
///
/// Created by [`Agent::builder`]. Without [`context`](Self::context) the agent keeps no
/// memory between turns.
pub struct AgentBuilder<'k, T> {
    model: Model,
    knowledge: &'k Knowledge<T>,
    max_targets: usize,
}

impl<T: fmt::Debug> fmt::Debug for AgentBuilder<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentBuilder")
            .field("model", &self.model)
            .field("knowledge", &self.knowledge)
            .field("max_targets", &self.max_targets)
            .finish()
    }
}

impl<'k, T> AgentBuilder<'k, T> {
    /// Enables conversation memory that keeps up to `max_targets` targets in each list.
    ///
    /// The limit applies to the choices of a pending clarification and to the targets of
    /// the last successful command. Zero disables the memory, which is the default.
    #[must_use]
    pub const fn context(mut self, max_targets: usize) -> Self {
        self.max_targets = max_targets;
        self
    }

    /// Builds the agent.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::ModelMismatch`] when the knowledge was not embedded with the
    /// builder's model, even if it is empty.
    pub fn build(self) -> Result<Agent<'k, T>, AgentError> {
        if self.model.same_as(self.knowledge.embedder().model()) {
            Ok(Agent {
                model: self.model,
                knowledge: self.knowledge,
                context: Context::new(self.max_targets),
            })
        } else {
            Err(AgentError::ModelMismatch)
        }
    }
}

impl<'k, T> Agent<'k, T> {
    /// Creates an agent that answers prompts from `knowledge` and keeps no conversation
    /// memory.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError::ModelMismatch`] when `knowledge` was not embedded with
    /// `model`, even if it is empty. Two models are the same when they come from the
    /// same [`model!`](crate::model!) call.
    pub fn new(model: Model, knowledge: &'k Knowledge<T>) -> Result<Self, AgentError> {
        Self::builder(model, knowledge).build()
    }

    /// Starts configuring an agent that answers prompts from `knowledge`.
    #[must_use]
    pub const fn builder(model: Model, knowledge: &'k Knowledge<T>) -> AgentBuilder<'k, T> {
        AgentBuilder {
            model,
            knowledge,
            max_targets: 0,
        }
    }

    /// Handles one line of conversation and runs the command it holds, if any.
    ///
    /// The handler's [`parse`](TurnHandler::parse) gets the original input and, as a hint,
    /// the payload of the closest stored text with a positive similarity. A parsed command
    /// supersedes any pending clarification. Its targets then resolve as follows:
    ///
    /// * A resolved, non-empty group is dispatched once, whatever its size.
    /// * An ambiguous match becomes the pending clarification when its choices fit the
    ///   context limit, and the outcome is [`TurnOutcome::Clarify`]. Otherwise the user must
    ///   give a more specific command and the outcome is [`TurnOutcome::NeedTarget`].
    /// * A [`TargetSelector::Previous`] reference resolves only to a single remembered
    ///   target. A remembered group becomes a pending clarification. With no remembered
    ///   target the outcome is [`TurnOutcome::NeedTarget`].
    ///
    /// Input that parses to no command answers the pending clarification when exactly one
    /// of its choices matches. Any other input leaves the clarification pending and repeats
    /// its choices. With nothing pending the outcome is [`TurnOutcome::Unrecognized`].
    ///
    /// A successful dispatch replaces the remembered targets, or clears them when the full
    /// target list exceeds the context limit. A failed one keeps the previous memory. With
    /// the memory disabled, explicit commands work and nothing is ever remembered.
    pub fn turn<H>(&mut self, input: &str, handler: &mut H) -> TurnOutcome<H::Success, H::Error>
    where
        H: TurnHandler<T>,
    {
        let suggested = self.suggestion(input);
        match handler.parse(input, suggested) {
            Some(command) => self.run_command(command, handler),
            None => self.answer(input, handler),
        }
    }

    /// Returns the payload closest to `input` when it is similar to it at all.
    fn suggestion(&self, input: &str) -> Option<&'k T> {
        self.prompt(input, 1)
            .into_iter()
            .find(|hit| hit.score > 0.0)
            .map(|hit| hit.payload)
    }

    /// Acts on a command that parsed, replacing any pending clarification.
    fn run_command<H>(
        &mut self,
        command: Command<'_, T>,
        handler: &mut H,
    ) -> TurnOutcome<H::Success, H::Error>
    where
        H: TurnHandler<T>,
    {
        self.context.pending = None;
        let matched = match command.target {
            TargetSelector::Explicit(selector) => handler.resolve_targets(selector),
            TargetSelector::Previous => match self.context.last.as_slice() {
                [] => TargetMatch::Missing,
                &[target] => return self.dispatch(command.action, &[target], handler),
                choices => TargetMatch::Ambiguous(choices.to_vec()),
            },
        };
        match matched {
            TargetMatch::Resolved(targets) if !targets.is_empty() => {
                self.dispatch(command.action, &targets, handler)
            }
            TargetMatch::Ambiguous(choices)
                if !choices.is_empty() && choices.len() <= self.context.max_targets =>
            {
                self.context.pending = Some(Box::new(Pending {
                    action: command.action,
                    choices: choices.clone(),
                }));
                TurnOutcome::Clarify(choices)
            }
            TargetMatch::Resolved(_) | TargetMatch::Ambiguous(_) | TargetMatch::Missing => {
                TurnOutcome::NeedTarget
            }
        }
    }

    /// Treats input that is not a command as the user's answer to a pending clarification.
    fn answer<H>(&mut self, input: &str, handler: &mut H) -> TurnOutcome<H::Success, H::Error>
    where
        H: TurnHandler<T>,
    {
        let Some(pending) = self.context.pending.take() else {
            return TurnOutcome::Unrecognized;
        };
        let mut matching = pending
            .choices
            .iter()
            .copied()
            .filter(|target| handler.matches_target(*target, input));
        if let (Some(target), None) = (matching.next(), matching.next()) {
            self.dispatch(pending.action, &[target], handler)
        } else {
            let choices = pending.choices.clone();
            self.context.pending = Some(pending);
            TurnOutcome::Clarify(choices)
        }
    }

    /// Runs `action` on `targets` and remembers them only if it succeeds and they fit.
    fn dispatch<H>(
        &mut self,
        action: T,
        targets: &[TargetId],
        handler: &mut H,
    ) -> TurnOutcome<H::Success, H::Error>
    where
        H: TurnHandler<T>,
    {
        match handler.dispatch(action, targets) {
            Ok(success) => {
                self.context.last.clear();
                if targets.len() <= self.context.max_targets {
                    self.context.last.extend_from_slice(targets);
                }
                TurnOutcome::Executed(success)
            }
            Err(error) => TurnOutcome::Failed(error),
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
