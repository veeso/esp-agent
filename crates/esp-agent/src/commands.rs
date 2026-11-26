//! The typed command parser that `#[derive(TurnCommands)]` generates, the vocabularies that
//! replace it at run time, and the handler adapter.

use core::fmt;

use crate::catalog::{TargetCatalog, TargetLookup, trim_target_suffix};
use crate::context::{Command, TargetId, TargetMatch, TargetSelector, TurnHandler};
use crate::runtime_vocabulary::RuntimeVocabulary;

/// An action enum whose commands are declared next to its variants.
///
/// Derive it with `#[derive(TurnCommands)]`. Each variant that carries one or more
/// `#[command("..")]` patterns can be spelled by the user. A pattern is made of literals
/// and captures in braces, and must match the whole input:
///
/// * `{target}` captures the non-empty text that names the devices. It becomes
///   [`TargetSelector::Explicit`], or [`TargetSelector::Previous`] when it equals the word
///   configured with `#[commands(previous = "it")]` on the enum.
/// * `{field}` captures the text of a named field of the variant, which is converted with
///   the field type's [`FromStr`](core::str::FromStr). A value it rejects means the input is
///   not that command. Each field appears exactly once.
///
/// Literals match ignoring ASCII case. Outer whitespace and trailing sentence punctuation
/// of the input are ignored, and so is whitespace around a capture. Two captures need a
/// literal between them. When a literal occurs inside the text of a capture, every split
/// of the input is tried, and input that more than one pattern or split accepts is
/// declined as ambiguous. Variants without `#[command]` are skipped, so handwritten code
/// can construct them.
///
/// The derive also implements [`NamedActions`](crate::NamedActions), so that a
/// [`RuntimeVocabulary`] can spell the same variants by name.
///
/// Matching borrows the input and allocates nothing. The field types' own parsers pay for
/// their costs. Pattern problems are compile errors, and they are also checked on the
/// build host.
///
/// ```rust,ignore
/// #[derive(TurnCommands)]
/// #[commands(previous = "it")]
/// enum Action {
///     #[command("turn on {target}", "turn {target} on")]
///     TurnOn,
///     #[command("dim {target} to {level}%", "dim {target} to {level} percent")]
///     SetBrightness { level: Percent },
///     // Built by handwritten code.
///     BeginDim,
/// }
/// ```
pub trait TurnCommands: Sized {
    /// Extracts the command that spells all of `input`, or returns `None`.
    ///
    /// This is the same parser that [`handler`](Self::handler) uses, so a handwritten
    /// [`TurnHandler::parse`] can call it first and then try input of its own.
    fn parse_command(input: &str) -> Option<Command<'_, Self>>;

    /// Builds a [`TurnHandler`] that parses with [`parse_command`](Self::parse_command),
    /// looks targets up in `catalog`, and forwards every dispatch to `dispatch`.
    ///
    /// The catalog is any [`TargetLookup`], such as a [`TargetCatalog`](crate::TargetCatalog)
    /// that `targets!` declares or a [`DynamicTargetCatalog`](crate::DynamicTargetCatalog)
    /// that firmware fills at run time.
    ///
    /// The stored text that the agent suggests is ignored, and the original input is always
    /// checked. The callback runs the action on the firmware's devices and its result is
    /// the turn's success or failure. For a dynamic catalog, the callback must reject IDs for
    /// devices that are no longer present: a previous-target command can reuse a remembered ID
    /// without a new lookup. Write a handler by hand when you need the suggestion.
    fn handler<F, S, E>(
        catalog: &(impl TargetLookup + ?Sized),
        dispatch: F,
    ) -> impl TurnHandler<Self, Success = S, Error = E>
    where
        F: FnMut(Self, &[TargetId]) -> Result<S, E>,
    {
        Handler {
            catalog,
            parse: Self::parse_command,
            dispatch,
        }
    }
}

type Parser<T> = for<'a> fn(&'a str) -> Option<Command<'a, T>>;

/// The phrases of one language that spell the commands of an action enum.
///
/// Declare a vocabulary with [`vocabulary!`](crate::vocabulary) or use the derived patterns
/// from [`Vocabulary::derived`]. A vocabulary is `Copy`, and the macro expands to a
/// constant expression, so firmware can keep one per language in a `static`.
///
/// The generated matcher borrows the input without allocating. Field types'
/// [`FromStr`](core::str::FromStr) implementations and custom parsers passed to
/// [`Vocabulary::new`] can allocate.
pub struct Vocabulary<T> {
    parse: Parser<T>,
}

impl<T> Vocabulary<T> {
    /// Wraps a parser that extracts the command spelled by all of its input.
    #[must_use]
    pub const fn new(parse: fn(&str) -> Option<Command<'_, T>>) -> Self {
        Self { parse }
    }

    /// Extracts the command that spells all of `input`, or returns `None`.
    #[must_use]
    pub fn parse<'a>(&self, input: &'a str) -> Option<Command<'a, T>> {
        (self.parse)(input)
    }
}

impl<T: TurnCommands> Vocabulary<T> {
    /// Wraps the action's [`TurnCommands::parse_command`] as a vocabulary.
    ///
    /// Pass it to [`CommandHandler::new`] to start with that parser, or to
    /// [`CommandHandler::set_vocabulary`] to restore it after switching languages.
    #[must_use]
    pub const fn derived() -> Self {
        Self::new(T::parse_command)
    }
}

impl<T> Clone for Vocabulary<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Vocabulary<T> {}

impl<T> fmt::Debug for Vocabulary<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Vocabulary").finish_non_exhaustive()
    }
}

/// The vocabulary that a [`CommandHandler`] parses with, compiled or built at run time.
///
/// Both a [`Vocabulary`] and a [`RuntimeVocabulary`] convert into it, so the methods of
/// [`CommandHandler`] accept either.
pub struct ActiveVocabulary<T>(Source<T>);

enum Source<T> {
    Compiled(Parser<T>),
    Runtime(RuntimeVocabulary<T>),
}

impl<T> ActiveVocabulary<T> {
    /// Extracts the command that spells all of `input`, or returns `None`.
    #[must_use]
    pub fn parse<'a>(&self, input: &'a str) -> Option<Command<'a, T>> {
        match &self.0 {
            Source::Compiled(parse) => parse(input),
            Source::Runtime(words) => words.parse(input),
        }
    }
}

impl<T> From<Vocabulary<T>> for ActiveVocabulary<T> {
    fn from(vocabulary: Vocabulary<T>) -> Self {
        Self(Source::Compiled(vocabulary.parse))
    }
}

impl<T> From<RuntimeVocabulary<T>> for ActiveVocabulary<T> {
    fn from(vocabulary: RuntimeVocabulary<T>) -> Self {
        Self(Source::Runtime(vocabulary))
    }
}

impl<T> fmt::Debug for ActiveVocabulary<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tuple = formatter.debug_tuple("ActiveVocabulary");
        match &self.0 {
            Source::Compiled(parse) => tuple.field(&Vocabulary::new(*parse)),
            Source::Runtime(words) => tuple.field(words),
        };
        tuple.finish()
    }
}

/// A [`TurnHandler`] whose vocabulary can change between turns.
///
/// It parses with its current [`ActiveVocabulary`], so firmware can switch the command
/// language between turns while the [`Agent`](crate::Agent) keeps the conversation memory.
pub struct CommandHandler<'c, T, F, C: ?Sized = TargetCatalog>(
    Handler<'c, C, ActiveVocabulary<T>, F>,
);

impl<'c, T, F, C: TargetLookup + ?Sized> CommandHandler<'c, T, F, C> {
    /// Creates a handler that parses with the supplied vocabulary.
    ///
    /// The vocabulary is a [`Vocabulary`] or a [`RuntimeVocabulary`].
    ///
    /// Like [`TurnCommands::handler`], it accepts any [`TargetLookup`], ignores the
    /// agent's suggestion, and forwards every dispatch to `dispatch`. The callback
    /// must reject removed devices, since a previous-target command can reuse a
    /// remembered ID without a new lookup.
    pub fn new<S, E>(
        catalog: &'c C,
        vocabulary: impl Into<ActiveVocabulary<T>>,
        dispatch: F,
    ) -> Self
    where
        F: FnMut(T, &[TargetId]) -> Result<S, E>,
    {
        Self(Handler {
            catalog,
            parse: vocabulary.into(),
            dispatch,
        })
    }

    /// Replaces the phrases the handler understands from the next turn on.
    ///
    /// The vocabulary replaces the previous one instead of adding to it, so phrases of the
    /// old language are no longer recognized. The agent keeps the remembered targets and
    /// any pending question, because they belong to the agent and not to the handler.
    /// A replaced [`RuntimeVocabulary`] is dropped, which frees its heap.
    pub fn set_vocabulary(&mut self, vocabulary: impl Into<ActiveVocabulary<T>>) {
        self.0.parse = vocabulary.into();
    }
}

impl<T, F, C: fmt::Debug + ?Sized> fmt::Debug for CommandHandler<'_, T, F, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandHandler")
            .field("catalog", &self.0.catalog)
            .field("vocabulary", &self.0.parse)
            .finish_non_exhaustive()
    }
}

impl<T, F, C, S, E> TurnHandler<T> for CommandHandler<'_, T, F, C>
where
    C: TargetLookup + ?Sized,
    F: FnMut(T, &[TargetId]) -> Result<S, E>,
{
    type Success = S;
    type Error = E;

    fn parse<'a>(&self, input: &'a str, suggested: Option<&T>) -> Option<Command<'a, T>> {
        self.0.parse(input, suggested)
    }

    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        self.0.resolve_targets(selector)
    }

    fn target_available(&self, target: TargetId) -> bool {
        self.0.target_available(target)
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        self.0.matches_target(target, reply)
    }

    fn dispatch(&mut self, action: T, targets: &[TargetId]) -> Result<S, E> {
        self.0.dispatch(action, targets)
    }
}

/// Shares catalog and dispatch handling, storing no pointer for a fixed function item.
struct Handler<'c, C: ?Sized, P, F> {
    catalog: &'c C,
    parse: P,
    dispatch: F,
}

/// What [`Handler`] parses with: a parser function or the vocabulary of a
/// [`CommandHandler`].
trait ParseInput<T> {
    fn parse_input<'a>(&self, input: &'a str) -> Option<Command<'a, T>>;
}

impl<T, P> ParseInput<T> for P
where
    P: for<'a> Fn(&'a str) -> Option<Command<'a, T>>,
{
    fn parse_input<'a>(&self, input: &'a str) -> Option<Command<'a, T>> {
        self(input)
    }
}

impl<T> ParseInput<T> for ActiveVocabulary<T> {
    fn parse_input<'a>(&self, input: &'a str) -> Option<Command<'a, T>> {
        self.parse(input)
    }
}

impl<C, T, P, F, S, E> TurnHandler<T> for Handler<'_, C, P, F>
where
    C: TargetLookup + ?Sized,
    P: ParseInput<T>,
    F: FnMut(T, &[TargetId]) -> Result<S, E>,
{
    type Success = S;
    type Error = E;

    fn parse<'a>(&self, input: &'a str, _suggested: Option<&T>) -> Option<Command<'a, T>> {
        self.parse.parse_input(input)
    }

    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        self.catalog.resolve_targets(selector)
    }

    fn target_available(&self, target: TargetId) -> bool {
        self.catalog.target_available(target)
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        self.catalog.matches_target(target, reply)
    }

    fn dispatch(&mut self, action: T, targets: &[TargetId]) -> Result<S, E> {
        (self.dispatch)(action, targets)
    }
}

/// Removes outer whitespace and trailing sentence punctuation.
#[must_use]
pub fn trim_sentence(text: &str) -> &str {
    text.trim().trim_end_matches(['.', '!', '?']).trim_end()
}

/// Removes leading whitespace and a trailing run of whitespace and sentence punctuation.
#[must_use]
pub fn trim_vocabulary(text: &str) -> &str {
    trim_target_suffix(text.trim_start())
}

/// Maps the text of a `{target}` capture to a selector.
#[must_use]
pub fn selector<'a>(target: &'a str, previous: Option<&str>) -> TargetSelector<'a> {
    if let Some(word) = previous
        && let Some((head, suffix)) = target.split_at_checked(word.len())
        && head.eq_ignore_ascii_case(word)
        && trim_target_suffix(suffix).is_empty()
    {
        TargetSelector::Previous
    } else {
        TargetSelector::Explicit(target)
    }
}

/// The generated parser's decision after validating a capture.
#[derive(Debug)]
pub enum CaptureMatch {
    /// Skip splits that start with this capture.
    Reject,
    /// Examine the next capture or another possible split.
    Accept,
    /// Stop because a second complete match was found.
    Ambiguous,
}

/// Visits captures in pattern order and returns whether the visitor found ambiguity.
///
/// The first and last literals anchor the input. Each interior literal delimits a capture.
/// At most eight captures can be declared, so the search needs bounded stack space.
pub fn split<'a>(
    input: &'a str,
    literals: &[&str],
    mut visit: impl FnMut(usize, &'a str) -> CaptureMatch,
) -> bool {
    let [first, inner @ .., last] = literals else {
        return false;
    };
    let Some(body) = strip_prefix(input, first).and_then(|rest| strip_suffix(rest, last)) else {
        return false;
    };
    // Each active capture remembers where it begins and where to resume looking for
    // its delimiter after visiting the next capture.
    let mut positions = [(0, 0); 8];
    if inner.len() >= positions.len() {
        return false;
    }
    let mut depth = 0;
    loop {
        let (start, cursor) = positions[depth];
        if let Some(delimiter) = inner.get(depth) {
            if let Some((offset, ch)) = body[cursor..]
                .char_indices()
                .find(|(offset, _)| starts_with(&body[cursor + offset..], delimiter))
            {
                let end = cursor + offset;
                positions[depth].1 = end + ch.len_utf8();
                let part = body[start..end].trim();
                if !part.is_empty() && matches!(visit(depth, part), CaptureMatch::Accept) {
                    depth += 1;
                    let next = end + delimiter.len();
                    positions[depth] = (next, next);
                }
                continue;
            }
        } else {
            let part = body[start..].trim();
            if !part.is_empty() && matches!(visit(depth, part), CaptureMatch::Ambiguous) {
                return true;
            }
        }
        let Some(parent) = depth.checked_sub(1) else {
            return false;
        };
        depth = parent;
    }
}

fn starts_with(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

fn strip_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with(text, prefix).then(|| &text[prefix.len()..])
}

fn strip_suffix<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let start = text.len().checked_sub(suffix.len())?;
    text.get(start..)?
        .eq_ignore_ascii_case(suffix)
        .then(|| &text[..start])
}
