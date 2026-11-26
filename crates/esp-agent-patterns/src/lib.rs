//! The rules of command patterns, shared by `esp-agent` and its macros.
//!
//! This crate is an internal part of `esp-agent`: depend on that crate instead. Its items
//! can change in any release. It exists so that patterns checked while the firmware
//! compiles and patterns checked while it runs follow one implementation.
//!
//! A pattern is made of literals and `{name}` captures, with `{{` and `}}` for literal
//! braces. Every allocation made here is a fallible reservation, so a caller that parses
//! text from the network gets an error instead of an abort when memory runs out.
#![no_std]

extern crate alloc;

use alloc::collections::TryReserveError;
use alloc::string::String;
use alloc::vec::Vec;

/// The most captures one pattern can hold, bounding the depth of the matching search.
#[doc(hidden)]
pub const MAX_CAPTURES: usize = 8;

/// The name that a pattern uses for the target of the command.
#[doc(hidden)]
pub const TARGET: &str = "target";

/// Which outer text a pattern loses, matching what its parser removes from the input.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Normalization {
    /// The rule of `#[derive(TurnCommands)]`: one run of sentence punctuation.
    Derive,
    /// The rule of vocabularies: any trailing run of whitespace and sentence punctuation.
    Vocabulary,
}

/// Whether a pattern must capture the target of the command.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRule {
    /// The pattern names its target with `{target}`.
    Required,
    /// The pattern refers to the previous target and holds no `{target}`.
    Forbidden,
}

/// One piece of a command pattern.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Piece {
    /// Text that the input must repeat, ignoring ASCII case.
    Literal(String),
    /// A named capture such as `{target}`.
    Capture(String),
}

/// A validated pattern: the literals around its captures and which name each capture has.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct Pattern {
    /// One more literal than there are captures. Only the first and last can be empty.
    pub literals: Vec<String>,
    /// The name of each capture, in order of appearance.
    pub captures: Vec<String>,
}

/// Why a pattern was refused.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternError<'a> {
    /// A `{` has no matching `}`.
    UnclosedCapture,
    /// A capture has no name.
    EmptyCapture,
    /// A `}` is neither the end of a capture nor part of `}}`.
    UnmatchedBrace,
    /// A memory reservation failed.
    OutOfMemory,
    /// Two captures have no literal between them.
    AdjacentCaptures,
    /// A capture names neither a field nor the target.
    UnknownField(&'a str),
    /// A pattern that refers to the previous target captures a target.
    TargetInPrevious,
    /// A capture appears twice.
    RepeatedCapture(&'a str),
    /// A pattern that needs a target has none.
    MissingTarget,
    /// A field is not captured.
    MissingField(&'a str),
    /// Nothing is left of the pattern after normalization.
    EmptyPhrase,
    /// The pattern has more than [`MAX_CAPTURES`] captures.
    TooManyCaptures,
}

impl From<TryReserveError> for PatternError<'_> {
    fn from(_: TryReserveError) -> Self {
        Self::OutOfMemory
    }
}

/// Removes the outer text that the parser of the pattern removes from its input.
///
/// A pattern that kept it could never match.
#[doc(hidden)]
#[must_use]
pub fn normalize(pattern: &str, normalization: Normalization) -> &str {
    match normalization {
        Normalization::Derive => pattern.trim().trim_end_matches(['.', '!', '?']).trim_end(),
        Normalization::Vocabulary => pattern
            .trim_start()
            .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?')),
    }
}

/// Splits a normalized pattern into literals and `{name}` captures.
///
/// It stops at the first capture past `max_captures`, which bounds the memory spent on a
/// pattern that cannot be accepted. A caller that wants [`validate`] to report the first
/// broken rule of any pattern passes [`usize::MAX`].
///
/// # Errors
///
/// Returns [`PatternError::UnclosedCapture`], [`PatternError::EmptyCapture`], or
/// [`PatternError::UnmatchedBrace`] for malformed braces,
/// [`PatternError::TooManyCaptures`] past `max_captures`, and
/// [`PatternError::OutOfMemory`] when a reservation fails.
#[doc(hidden)]
pub fn parse_pieces(
    pattern: &str,
    normalization: Normalization,
    max_captures: usize,
) -> Result<Vec<Piece>, PatternError<'_>> {
    let mut pieces = Vec::new();
    let mut captures = 0;
    let mut literal = String::new();
    let mut chars = normalize(pattern, normalization).chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' if chars.peek() == Some(&c) => {
                chars.next();
                push_char(&mut literal, c)?;
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => push_char(&mut name, c)?,
                        None => return Err(PatternError::UnclosedCapture),
                    }
                }
                if name.is_empty() {
                    return Err(PatternError::EmptyCapture);
                }
                if !literal.is_empty() {
                    push(&mut pieces, Piece::Literal(core::mem::take(&mut literal)))?;
                }
                if captures == max_captures {
                    return Err(PatternError::TooManyCaptures);
                }
                captures += 1;
                push(&mut pieces, Piece::Capture(name))?;
            }
            '}' => return Err(PatternError::UnmatchedBrace),
            c => push_char(&mut literal, c)?,
        }
    }
    if !literal.is_empty() {
        push(&mut pieces, Piece::Literal(literal))?;
    }
    Ok(pieces)
}

/// Checks the captures of one pattern against the fields of its action.
///
/// # Errors
///
/// Returns the first broken rule. The captures are checked in order of appearance, then
/// the pattern as a whole. Returns [`PatternError::OutOfMemory`] when a reservation fails.
///
/// # Panics
///
/// Never panics: the literal list starts with one entry and only grows.
#[doc(hidden)]
pub fn validate<'a>(
    pieces: &'a [Piece],
    fields: &[&'a str],
    rule: TargetRule,
) -> Result<Pattern, PatternError<'a>> {
    let mut literals = Vec::new();
    push(&mut literals, String::new())?;
    let mut captures: Vec<String> = Vec::new();
    let mut after_capture = false;
    for piece in pieces {
        match piece {
            Piece::Literal(text) => {
                push_str(literals.last_mut().expect("there is a literal"), text)?;
                after_capture = false;
            }
            Piece::Capture(name) => {
                let name = name.as_str();
                if after_capture {
                    return Err(PatternError::AdjacentCaptures);
                }
                if name != TARGET && !fields.contains(&name) {
                    return Err(PatternError::UnknownField(name));
                }
                if name == TARGET && rule == TargetRule::Forbidden {
                    return Err(PatternError::TargetInPrevious);
                }
                if captures.iter().any(|capture| capture == name) {
                    return Err(PatternError::RepeatedCapture(name));
                }
                let mut capture = String::new();
                push_str(&mut capture, name)?;
                push(&mut captures, capture)?;
                push(&mut literals, String::new())?;
                after_capture = true;
            }
        }
    }
    let captured = |name: &str| captures.iter().any(|capture| capture == name);
    if rule == TargetRule::Required && !captured(TARGET) {
        return Err(PatternError::MissingTarget);
    }
    if let Some(&field) = fields.iter().find(|field| !captured(field)) {
        return Err(PatternError::MissingField(field));
    }
    if captures.is_empty() && literals[0].is_empty() {
        return Err(PatternError::EmptyPhrase);
    }
    if captures.len() > MAX_CAPTURES {
        return Err(PatternError::TooManyCaptures);
    }
    Ok(Pattern { literals, captures })
}

/// Returns whether two patterns accept the same input in the same way.
///
/// Literals are compared ignoring ASCII case, and capture names exactly.
#[doc(hidden)]
#[must_use]
pub fn same_pattern(a: &Pattern, b: &Pattern) -> bool {
    a.captures == b.captures
        && a.literals.len() == b.literals.len()
        && a.literals
            .iter()
            .zip(&b.literals)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), TryReserveError> {
    items.try_reserve(1)?;
    items.push(item);
    Ok(())
}

fn push_char(text: &mut String, c: char) -> Result<(), TryReserveError> {
    text.try_reserve(c.len_utf8())?;
    text.push(c);
    Ok(())
}

/// Appends with an exact reservation, so that a stored string holds no spare room.
fn push_str(text: &mut String, suffix: &str) -> Result<(), TryReserveError> {
    text.try_reserve_exact(suffix.len())?;
    text.push_str(suffix);
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::*;

    fn rejected(pattern: &str, fields: &[&str], rule: TargetRule) -> String {
        let pieces = match parse_pieces(pattern, Normalization::Vocabulary, usize::MAX) {
            Ok(pieces) => pieces,
            Err(error) => return alloc::format!("{error:?}"),
        };
        let error = validate(&pieces, fields, rule).expect_err("the pattern is invalid");
        alloc::format!("{error:?}")
    }

    fn pattern(text: &str, fields: &[&str]) -> Pattern {
        let pieces = parse_pieces(text, Normalization::Vocabulary, usize::MAX).unwrap();
        validate(&pieces, fields, TargetRule::Required).unwrap()
    }

    #[test]
    fn patterns_split_into_literals_and_captures() {
        let pieces = parse_pieces(
            " Dim {target} to {{{level}}}%!? ",
            Normalization::Vocabulary,
            usize::MAX,
        )
        .unwrap();
        let pattern = validate(&pieces, &["level"], TargetRule::Required).unwrap();
        assert_eq!(pattern.literals, ["Dim ", " to {", "}%"]);
        assert_eq!(pattern.captures, ["target", "level"]);
    }

    #[test]
    fn captures_at_the_ends_have_empty_literals() {
        let pattern = pattern("{target}", &[]);
        assert_eq!(pattern.literals, ["", ""]);
        assert_eq!(pattern.captures, ["target"]);
    }

    #[test]
    fn normalizations_differ_only_in_trailing_whitespace_runs() {
        assert_eq!(
            normalize("spegnila! ?", Normalization::Vocabulary),
            "spegnila"
        );
        assert_eq!(normalize("spegnila! ?", Normalization::Derive), "spegnila!");
        for normalization in [Normalization::Vocabulary, Normalization::Derive] {
            assert_eq!(normalize("  spegnila!? ", normalization), "spegnila");
        }
    }

    #[test]
    fn invalid_patterns_report_each_rule() {
        use TargetRule::{Forbidden, Required};

        let nine = ["a", "b", "c", "d", "e", "f", "g", "h"];
        for (text, fields, rule, expected) in [
            ("a {target", &[][..], Required, "UnclosedCapture"),
            ("a {}", &[], Required, "EmptyCapture"),
            ("a } b {target}", &[], Required, "UnmatchedBrace"),
            ("{target}{level}", &["level"], Required, "AdjacentCaptures"),
            ("a {nope} {target}", &[], Required, "UnknownField(\"nope\")"),
            ("a {target}", &[], Forbidden, "TargetInPrevious"),
            (
                "{target} a {target}",
                &[],
                Required,
                "RepeatedCapture(\"target\")",
            ),
            ("a", &[], Required, "MissingTarget"),
            (
                "a {target}",
                &["level"],
                Required,
                "MissingField(\"level\")",
            ),
            (" .!? ", &[], Forbidden, "EmptyPhrase"),
            (
                "{target} {a} {b} {c} {d} {e} {f} {g} {h}",
                &nine,
                Required,
                "TooManyCaptures",
            ),
        ] {
            assert_eq!(rejected(text, fields, rule), expected, "{text}");
        }
    }

    #[test]
    fn same_pattern_ignores_ascii_case_of_literals_only() {
        assert!(same_pattern(
            &pattern("Spegni {target}", &[]),
            &pattern("spegni {target}", &[])
        ));
        assert!(!same_pattern(
            &pattern("a {target} {level}", &["level"]),
            &pattern("a {level} {target}", &["level"])
        ));
        assert!(!same_pattern(
            &pattern("spegni {target}", &[]),
            &pattern("spegni {target} ora", &[])
        ));
        assert!(!same_pattern(
            &pattern("così {target}", &[]),
            &pattern("COSÌ {target}", &[])
        ));
    }

    #[test]
    fn parsing_stops_at_the_capture_past_the_limit() {
        let parse = |text| parse_pieces(text, Normalization::Vocabulary, 2);
        assert!(parse("{a}x{b}x").is_ok());
        // The rest of the pattern is not read, so its unclosed brace goes unnoticed.
        assert_eq!(parse("{a}x{b}x{c}x{"), Err(PatternError::TooManyCaptures));
    }

    #[test]
    fn parsing_many_captures_keeps_memory_linear() {
        // Every stored string reserves what it holds, so many captures stay linear.
        let text = "ab{x}".repeat(100);
        let pieces = parse_pieces(&text, Normalization::Vocabulary, usize::MAX).unwrap();
        let held: usize = pieces
            .iter()
            .map(|piece| match piece {
                Piece::Literal(text) | Piece::Capture(text) => text.capacity(),
            })
            .sum();
        assert!(held <= text.len() * 8, "{held}");
        assert_eq!(pieces.len(), 200);
    }
}
