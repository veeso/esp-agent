//! Vocabularies that firmware builds from strings it receives at run time.

use alloc::vec::Vec;
use core::fmt;

use esp_agent_patterns::{
    MAX_CAPTURES, Normalization, Pattern, PatternError, TARGET, TargetRule, parse_pieces,
    same_pattern, validate,
};

use crate::commands::{CaptureMatch, selector, split, trim_vocabulary};
use crate::context::{Command, TargetSelector};

/// An action enum whose variants a run-time vocabulary can spell by name.
///
/// `#[derive(TurnCommands)]` implements it next to [`TurnCommands`](crate::TurnCommands),
/// so firmware writes no table from names to variants. The exposed actions are exactly the
/// variants that carry at least one `#[command]`. A variant without one stays reserved for
/// handwritten code, and text from the network cannot reach it.
///
/// The names and fields are the contract between the firmware and whatever sends it the
/// phrases, so renaming a variant or a field breaks vocabularies written for the old name.
pub trait NamedActions: Sized {
    /// Every action a run-time vocabulary can spell, in declaration order.
    const ACTIONS: &'static [ActionSpec];

    /// Checks whether an intermediate capture can be passed to [`Self::build`].
    ///
    /// `action` indexes [`Self::ACTIONS`] and `field` indexes [`ActionSpec::fields`].
    /// Return `false` only when this text makes the action impossible. The default defers
    /// validation to `build`. The derive checks the field's [`FromStr`](core::str::FromStr).
    fn accepts_field(_action: usize, _field: usize, _text: &str) -> bool {
        true
    }

    /// Matches a stored phrase using the action's field converters.
    #[doc(hidden)]
    fn match_phrase(action: usize, matcher: RuntimeMatch<'_, '_, Self>) -> bool {
        let count = Self::ACTIONS[action].fields.len();
        matcher.walk(
            [""; MAX_CAPTURES],
            |fields, index, text, last| {
                fields[index] = text;
                last || Self::accepts_field(action, index, text)
            },
            |fields, _| Self::build(action, &fields[..count]).ok_or(CaptureMatch::Reject),
        )
    }

    /// Builds the action at index `action` of [`ACTIONS`](Self::ACTIONS) from the text of
    /// its fields, given in the order of [`ActionSpec::fields`].
    ///
    /// Returns `None` when a field's [`FromStr`](core::str::FromStr) rejects its text, when
    /// `action` is not an index of [`ACTIONS`](Self::ACTIONS), or when the number of fields
    /// does not match the action. It never panics.
    fn build(action: usize, fields: &[&str]) -> Option<Self>;
}

/// The names of one action and of its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSpec {
    /// The variant identifier without `r#`, compared case-sensitively.
    pub name: &'static str,
    /// The field names in declaration order, spelled as a pattern captures them.
    pub fields: &'static [&'static str],
}

/// How much a [`RuntimeVocabulary`] may hold.
///
/// The default of 256 phrases and 16 KiB holds a device language many times over.
/// [`max_bytes`](Self::max_bytes) counts phrase text only, not the heap it takes: each
/// stored phrase also costs bookkeeping, and a short one can occupy a few hundred bytes.
/// Choose [`max_phrases`](Self::max_phrases) for the heap the firmware can spare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VocabularyLimits {
    /// The most phrases.
    pub max_phrases: usize,
    /// The most bytes of phrase text, counting each phrase as it was passed in. This is
    /// not a bound on heap use.
    pub max_bytes: usize,
}

impl Default for VocabularyLimits {
    fn default() -> Self {
        Self {
            max_phrases: 256,
            max_bytes: 16 * 1024,
        }
    }
}

/// Why a [`RuntimeVocabulary`] refused a phrase.
///
/// These are the checks that [`vocabulary!`](crate::vocabulary) makes while the firmware
/// compiles, and the failures that only exist at run time. A refused insertion leaves the
/// vocabulary unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocabularyError {
    /// No action has this name.
    UnknownAction,
    /// A `{name}` is neither a field of the action nor `{target}`.
    UnknownField,
    /// An action field uses the reserved Rust identifier `target`.
    ReservedField,
    /// The action declares the same field name twice.
    DuplicateField,
    /// A field of the action is not captured.
    MissingField,
    /// An explicit phrase has no `{target}`.
    MissingTarget,
    /// A previous phrase has a `{target}`.
    TargetInPrevious,
    /// A capture appears twice.
    RepeatedCapture,
    /// Two captures have no literal between them.
    AdjacentCaptures,
    /// A `{` has no matching `}`, or a capture has no name.
    MalformedCapture,
    /// A `}` is neither the end of a capture nor part of `}}`.
    UnmatchedBrace,
    /// Nothing is left of the phrase after normalization.
    EmptyPhrase,
    /// The phrase has more than eight captures, or its action more than eight fields.
    TooManyCaptures,
    /// The vocabulary already holds this phrase, ignoring ASCII case.
    DuplicatePattern,
    /// The phrase would exceed the [`VocabularyLimits`].
    TooLarge,
    /// A memory reservation failed.
    OutOfMemory,
}

impl VocabularyError {
    fn from_pattern(error: PatternError<'_>) -> Self {
        match error {
            PatternError::UnclosedCapture | PatternError::EmptyCapture => Self::MalformedCapture,
            PatternError::UnmatchedBrace => Self::UnmatchedBrace,
            PatternError::OutOfMemory => Self::OutOfMemory,
            PatternError::AdjacentCaptures => Self::AdjacentCaptures,
            PatternError::UnknownField(_) => Self::UnknownField,
            PatternError::TargetInPrevious => Self::TargetInPrevious,
            PatternError::RepeatedCapture(_) => Self::RepeatedCapture,
            PatternError::MissingTarget => Self::MissingTarget,
            PatternError::MissingField(_) => Self::MissingField,
            PatternError::EmptyPhrase => Self::EmptyPhrase,
            PatternError::TooManyCaptures => Self::TooManyCaptures,
        }
    }
}

impl fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownAction => "no action has this name",
            Self::UnknownField => "a capture names neither a field of the action nor the target",
            Self::ReservedField => "the field name `target` is reserved for the {target} capture",
            Self::DuplicateField => "the action declares a field twice",
            Self::MissingField => "the phrase does not capture every field of the action",
            Self::MissingTarget => "the phrase needs a {target} capture",
            Self::TargetInPrevious => {
                "a previous phrase refers to the previous target and cannot hold {target}"
            }
            Self::RepeatedCapture => "a capture appears twice",
            Self::AdjacentCaptures => "two captures need a literal between them",
            Self::MalformedCapture => "a capture is not closed or has no name",
            Self::UnmatchedBrace => "unmatched `}`, write `}}` for a literal brace",
            Self::EmptyPhrase => "the phrase is empty",
            Self::TooManyCaptures => "a pattern holds at most 8 captures",
            Self::DuplicatePattern => "the vocabulary already holds this phrase",
            Self::TooLarge => "the phrase exceeds the limits of the vocabulary",
            Self::OutOfMemory => "not enough memory to add the phrase",
        })
    }
}

impl core::error::Error for VocabularyError {}

/// What one capture of a stored phrase holds.
#[derive(Debug, Clone, Copy)]
enum Slot {
    /// The text that names the devices.
    Target,
    /// The field at this index of [`ActionSpec::fields`].
    Field(usize),
}

/// One validated phrase.
#[derive(Debug, Clone)]
struct Phrase {
    /// The index of the action in the action table.
    action: usize,
    pattern: Pattern,
    /// What each capture of the pattern holds. Entries past the last capture are unused.
    slots: [Slot; MAX_CAPTURES],
}

/// The phrases of one language, built from strings while the firmware runs.
///
/// This is the owned counterpart of [`Vocabulary`](crate::Vocabulary), for phrases that a
/// coordinator sends. Each insertion is checked with the rules that
/// [`vocabulary!`](crate::vocabulary) applies at build time, and the vocabulary matches
/// input like a compiled one with the same phrases.
///
/// Insertions allocate, only with fallible reservations, so a phrase that does not fit in
/// memory returns [`VocabularyError::OutOfMemory`] instead of aborting. Cloning allocates
/// like any other collection and does abort when memory runs out.
///
/// Build a whole language first, and install it with
/// [`CommandHandler::set_vocabulary`](crate::CommandHandler::set_vocabulary) only if every
/// insertion succeeded.
pub struct RuntimeVocabulary<T> {
    actions: &'static [ActionSpec],
    match_phrase: fn(usize, RuntimeMatch<'_, '_, T>) -> bool,
    phrases: Vec<Phrase>,
    limits: VocabularyLimits,
    bytes: usize,
}

impl<T: NamedActions> RuntimeVocabulary<T> {
    /// Creates a vocabulary without phrases and with the default [`VocabularyLimits`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(VocabularyLimits::default())
    }

    /// Creates a vocabulary without phrases that holds at most what `limits` allow.
    #[must_use]
    pub fn with_limits(limits: VocabularyLimits) -> Self {
        Self {
            actions: T::ACTIONS,
            match_phrase: T::match_phrase,
            phrases: Vec::new(),
            limits,
            bytes: 0,
        }
    }
}

impl<T> RuntimeVocabulary<T> {
    /// Adds a phrase of `action` that names its devices with `{target}`.
    ///
    /// `action` is the [`ActionSpec::name`] of a variant. The phrase follows the rules of an
    /// `explicit` phrase of [`vocabulary!`](crate::vocabulary).
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError::TooLarge`] when the phrase exceeds the limits,
    /// [`VocabularyError::UnknownAction`] for an unknown name,
    /// [`VocabularyError::DuplicatePattern`] for a phrase the vocabulary already holds,
    /// [`VocabularyError::OutOfMemory`] when a reservation fails, and the other
    /// [`VocabularyError`] variants for a phrase that breaks a pattern rule. The vocabulary
    /// is unchanged after an error.
    pub fn insert_explicit(&mut self, action: &str, pattern: &str) -> Result<(), VocabularyError> {
        self.insert(action, pattern, TargetRule::Required)
    }

    /// Adds a phrase of `action` that refers to the previous target without naming it.
    ///
    /// `action` is the [`ActionSpec::name`] of a variant. The phrase follows the rules of a
    /// `previous` phrase of [`vocabulary!`](crate::vocabulary), so it has no `{target}`.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`insert_explicit`](Self::insert_explicit). The vocabulary is
    /// unchanged after an error.
    pub fn insert_previous(&mut self, action: &str, pattern: &str) -> Result<(), VocabularyError> {
        self.insert(action, pattern, TargetRule::Forbidden)
    }

    /// Extracts the command that spells all of `input`, or returns `None`.
    ///
    /// Matching follows [`vocabulary!`](crate::vocabulary): literals ignore ASCII case,
    /// leading whitespace and a trailing run of whitespace and sentence punctuation are
    /// ignored, and input that more than one phrase or split accepts is declined. It borrows
    /// the input and allocates nothing apart from the field types'
    /// [`FromStr`](core::str::FromStr).
    #[must_use]
    pub fn parse<'a>(&self, input: &'a str) -> Option<Command<'a, T>> {
        let input = trim_vocabulary(input);
        let mut found = None;
        for phrase in &self.phrases {
            if (self.match_phrase)(
                phrase.action,
                RuntimeMatch {
                    phrase,
                    input,
                    found: &mut found,
                },
            ) {
                return None;
            }
        }
        found
    }

    /// Returns the number of phrases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.phrases.len()
    }

    /// Returns whether the vocabulary has no phrase, and therefore recognizes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.phrases.is_empty()
    }

    /// Returns what the vocabulary may hold.
    #[must_use]
    pub fn limits(&self) -> VocabularyLimits {
        self.limits
    }

    /// Returns the bytes of phrase text that count against [`VocabularyLimits::max_bytes`].
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    fn insert(
        &mut self,
        action: &str,
        pattern: &str,
        rule: TargetRule,
    ) -> Result<(), VocabularyError> {
        // The limits come first, so an oversized payload is refused without touching the heap.
        let bytes = self.bytes.saturating_add(pattern.len());
        if self.phrases.len() >= self.limits.max_phrases || bytes > self.limits.max_bytes {
            return Err(VocabularyError::TooLarge);
        }
        let (action, spec) = self
            .actions
            .iter()
            .enumerate()
            .find(|(_, spec)| spec.name == action)
            .ok_or(VocabularyError::UnknownAction)?;
        // Every field is captured once, so such an action cannot be spelled. Refusing it
        // here bounds the field texts that matching collects.
        if spec.fields.len() > MAX_CAPTURES {
            return Err(VocabularyError::TooManyCaptures);
        }
        let field_names = spec
            .fields
            .iter()
            .map(|field| field.strip_prefix("r#").unwrap_or(field));
        if field_names.clone().any(|name| name == TARGET) {
            return Err(VocabularyError::ReservedField);
        }
        if field_names
            .clone()
            .enumerate()
            .any(|(i, name)| field_names.clone().take(i).any(|prior| prior == name))
        {
            return Err(VocabularyError::DuplicateField);
        }
        let pieces = parse_pieces(pattern, Normalization::Vocabulary, MAX_CAPTURES)
            .map_err(VocabularyError::from_pattern)?;
        let pattern =
            validate(&pieces, spec.fields, rule).map_err(VocabularyError::from_pattern)?;
        if self
            .phrases
            .iter()
            .any(|phrase| same_pattern(&phrase.pattern, &pattern))
        {
            return Err(VocabularyError::DuplicatePattern);
        }
        let mut slots = [Slot::Target; MAX_CAPTURES];
        for (slot, capture) in slots.iter_mut().zip(&pattern.captures) {
            if capture != TARGET {
                let field = spec.fields.iter().position(|field| field == capture);
                *slot = Slot::Field(field.expect("a validated capture names a field"));
            }
        }
        self.phrases
            .try_reserve(1)
            .map_err(|_| VocabularyError::OutOfMemory)?;
        self.phrases.push(Phrase {
            action,
            pattern,
            slots,
        });
        self.bytes = bytes;
        Ok(())
    }
}

/// One stored phrase and the input and result of the current search.
#[doc(hidden)]
#[derive(Debug)]
pub struct RuntimeMatch<'p, 'a, T> {
    phrase: &'p Phrase,
    input: &'a str,
    found: &'p mut Option<Command<'a, T>>,
}

impl<'a, T> RuntimeMatch<'_, 'a, T> {
    /// Matches derived fields in pattern order, moving them into the first complete action.
    pub fn typed<S>(
        self,
        fields: S,
        mut parse: impl FnMut(&mut S, usize, &'a str) -> bool,
        build: impl Fn(&mut S) -> T,
    ) -> bool {
        self.walk(
            fields,
            |fields, index, text, _| parse(fields, index, text),
            |fields, found| {
                if found {
                    Err(CaptureMatch::Ambiguous)
                } else {
                    Ok(build(fields))
                }
            },
        )
    }

    fn walk<S>(
        self,
        mut fields: S,
        mut parse: impl FnMut(&mut S, usize, &'a str, bool) -> bool,
        mut build: impl FnMut(&mut S, bool) -> Result<T, CaptureMatch>,
    ) -> bool {
        let Self {
            phrase,
            input,
            found,
        } = self;
        let captures = phrase.pattern.captures.len();
        let mut target = TargetSelector::Previous;
        let mut complete = |fields: &mut S, target| {
            let action = match build(fields, found.is_some()) {
                Ok(action) => action,
                Err(result) => return result,
            };
            if found.is_some() {
                return CaptureMatch::Ambiguous;
            }
            *found = Some(Command { action, target });
            CaptureMatch::Accept
        };
        if captures == 0 {
            return input.eq_ignore_ascii_case(&phrase.pattern.literals[0])
                && matches!(complete(&mut fields, target), CaptureMatch::Ambiguous);
        }
        let mut literals = [""; MAX_CAPTURES + 1];
        for (slot, literal) in literals.iter_mut().zip(&phrase.pattern.literals) {
            *slot = literal;
        }
        split(input, &literals[..=captures], |index, part| {
            let last = index + 1 == captures;
            match phrase.slots[index] {
                Slot::Target => target = selector(part, None),
                Slot::Field(field) if !parse(&mut fields, field, part, last) => {
                    return CaptureMatch::Reject;
                }
                Slot::Field(_) => {}
            }
            if last {
                complete(&mut fields, target)
            } else {
                CaptureMatch::Accept
            }
        })
    }
}

impl<T: NamedActions> Default for RuntimeVocabulary<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for RuntimeVocabulary<T> {
    fn clone(&self) -> Self {
        Self {
            actions: self.actions,
            match_phrase: self.match_phrase,
            phrases: self.phrases.clone(),
            limits: self.limits,
            bytes: self.bytes,
        }
    }
}

impl<T> fmt::Debug for RuntimeVocabulary<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeVocabulary")
            .field("phrases", &self.phrases.len())
            .field("bytes", &self.bytes)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}
