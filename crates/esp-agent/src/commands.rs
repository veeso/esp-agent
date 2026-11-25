//! The typed command parser that `#[derive(TurnCommands)]` generates and its handler adapter.

use core::marker::PhantomData;

use crate::catalog::{TargetLookup, trim_target_suffix};
use crate::context::{Command, TargetId, TargetMatch, TargetSelector, TurnHandler};

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
        CommandHandler {
            catalog,
            dispatch,
            action: PhantomData,
        }
    }
}

/// The adapter that [`TurnCommands::handler`] returns.
struct CommandHandler<'c, C: ?Sized, T, F> {
    catalog: &'c C,
    dispatch: F,
    action: PhantomData<fn(T)>,
}

impl<C, T, F, S, E> TurnHandler<T> for CommandHandler<'_, C, T, F>
where
    C: TargetLookup + ?Sized,
    T: TurnCommands,
    F: FnMut(T, &[TargetId]) -> Result<S, E>,
{
    type Success = S;
    type Error = E;

    fn parse<'a>(&self, input: &'a str, _suggested: Option<&T>) -> Option<Command<'a, T>> {
        T::parse_command(input)
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
