//! The conversation state of an agent and the types of a turn.

use alloc::boxed::Box;
use alloc::vec::Vec;

/// A stable identifier of one device that firmware can act on.
///
/// The firmware assigns identifiers from its own catalog. They are the only form in which
/// the library keeps targets between turns, so the handler resolves them back to names.
pub type TargetId = u32;

/// How a command names the targets it acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSelector<'a> {
    /// A name, an alias, or a group such as a room, taken from the user's text.
    Explicit(&'a str),
    /// A singular reference such as "it" to the targets of the last successful command.
    Previous,
}

/// A command a [`TurnHandler`] extracted from the user's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command<'a, T> {
    /// The action to run, with its arguments.
    pub action: T,
    /// The targets the action applies to.
    pub target: TargetSelector<'a>,
}

/// The catalog entries a selector refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetMatch {
    /// The selector names exactly these targets, which the command acts on together.
    Resolved(Vec<TargetId>),
    /// The selector could name any one of these targets, so the user must choose.
    Ambiguous(Vec<TargetId>),
    /// The selector names nothing in the catalog.
    Missing,
}

/// The result of one [`Agent::turn`](crate::Agent::turn).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnOutcome<Success, Error> {
    /// The handler ran the action and returned this result.
    Executed(Success),
    /// The handler ran the action and it failed. The last successful targets stay remembered.
    Failed(Error),
    /// The user must choose one of these targets, and the next turn can answer.
    Clarify(Vec<TargetId>),
    /// The user must give a complete command naming a more specific target.
    NeedTarget,
    /// The text is neither a command nor an answer to a pending question.
    Unrecognized,
}

/// The firmware side of a conversational [`Agent`](crate::Agent): its vocabulary, its
/// catalog of targets, and the calls that act on them.
///
/// The library owns the order of events in a turn and the memory between turns. The
/// handler owns every decision about meaning. A similarity hit from the knowledge store
/// is only a suggestion, so `parse` must check the original input, including every
/// argument, before it returns a command. The handler gives each target one distinct
/// [`TargetId`] and `dispatch` must reject an identifier that the current catalog no
/// longer holds.
pub trait TurnHandler<T> {
    /// What a successful dispatch returns.
    type Success;
    /// What a failed dispatch returns.
    type Error;

    /// Extracts a command from `input`, or returns `None` if it is not one.
    ///
    /// `suggested` is the payload of the stored text closest to the input when its
    /// similarity is above zero. It is a hint, never an authorization.
    fn parse<'a>(&self, input: &'a str, suggested: Option<&T>) -> Option<Command<'a, T>>;

    /// Looks up the individual devices that `selector` names.
    ///
    /// For a group such as a room, return [`TargetMatch::Resolved`] with one ID for
    /// each matching device. The agent uses these IDs as choices for later clarification.
    fn resolve_targets(&self, selector: &str) -> TargetMatch;

    /// Returns whether `target` can still be offered in a clarification.
    ///
    /// The agent asks before offering remembered group members or matching a reply, and
    /// drops any choice for which this returns `false`. The default keeps every choice.
    /// This does not replace validation in [`Self::dispatch`].
    fn target_available(&self, _target: TargetId) -> bool {
        true
    }

    /// Tells whether `reply` picks `target` out of the choices a clarification offered.
    fn matches_target(&self, target: TargetId, reply: &str) -> bool;

    /// Runs `action` once on all of `targets`.
    ///
    /// # Errors
    ///
    /// Returns the handler's error when the action cannot run. The agent then remembers
    /// no new targets.
    fn dispatch(&mut self, action: T, targets: &[TargetId]) -> Result<Self::Success, Self::Error>;
}

/// A command waiting for the user to choose among its possible targets.
#[derive(Debug, Clone)]
pub(crate) struct Pending<T> {
    pub(crate) action: T,
    pub(crate) choices: Vec<TargetId>,
}

/// The bounded memory of one conversation.
///
/// Holds at most `max_targets` choices of one pending command and `max_targets` targets
/// of the last successful command. Only identifiers and structured arguments are kept.
#[derive(Debug, Clone)]
pub(crate) struct Context<T> {
    pub(crate) max_targets: usize,
    pub(crate) pending: Option<Box<Pending<T>>>,
    pub(crate) last: Vec<TargetId>,
}

impl<T> Context<T> {
    /// Creates an empty context that keeps up to `max_targets` targets per list.
    pub(crate) const fn new(max_targets: usize) -> Self {
        Self {
            max_targets,
            pending: None,
            last: Vec::new(),
        }
    }
}
