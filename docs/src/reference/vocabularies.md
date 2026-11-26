# Vocabularies

A vocabulary is the parser for one language of an action enum `T`: a
`Vocabulary<T>` compiled into the firmware, or a `RuntimeVocabulary<T>` built
while it runs. A `CommandHandler` parses with one vocabulary at a time, and
firmware replaces it with `set_vocabulary` while a conversation is running. The
[Switching languages](../languages.md) guide shows it in use.

## vocabulary!

`vocabulary!` evaluates to a `Vocabulary<T>`. Each entry names an enum variant
and one or two clauses. Separate entries with semicolons; the final semicolon
is optional.

```text
ENTRY := Enum::Variant [ { field: Type, .. } ] => CLAUSE [, CLAUSE]
CLAUSE := explicit ["pattern", ..] | previous ["phrase", ..]
```

```rust
{{#include ../../../crates/esp-agent/tests/support/italian_vocabulary.rs:italian}}
```

| Clause     | Needs                | Selector                   |
| ---------- | -------------------- | -------------------------- |
| `explicit` | A `{target}` capture | `TargetSelector::Explicit` |
| `previous` | No `{target}`        | `TargetSelector::Previous` |

An entry holds each clause at most once, in either order, and each list needs at
least one string. A variant with fields lists them with their types, so a
`{field}` capture can be converted with the type's `core::str::FromStr`. Every
field appears exactly once in each pattern. A `previous` phrase can consist
solely of a typed capture, such as `"{level}"`.

The expansion is a constant expression. It can initialize a `static`, or be passed
straight to `set_vocabulary`. Spell the enum path identically in every entry:
don't mix `Action::On` with `crate::Action::Off`. Put generic arguments on the
enum, as in `Dim::<u8>::Set`, not on the variant. In a generic function the
arguments can use the function's parameters, as in `Dim::<T>::Set`.

If a nested macro in an enum path or field type emits an unqualified name that
conflicts with the generated parser's input binding, define a type alias outside
`vocabulary!` and use that alias in the declaration. For example, define
`type Selected = Action<{ setting!() }>;` and then write `Selected::Off`.

A vocabulary has no previous word like `#[commands(previous = "it")]`. The text
that `{target}` captures is always an explicit name, so “accendi lo” selects a
target called “lo”. Write follow-ups as `previous` phrases instead.

## Matching

- The whole input must match after trimming leading whitespace and any run of
  trailing whitespace, `.`, `!`, and `?`. The declared phrases are trimmed the
  same way. “ SPEGNILA! ? ” therefore matches `"spegnila"`.
- Literals fold ASCII case only. “Basta così” matches `"basta così"`, but “BASTA
  COSÌ” does not, because `Ì` is not the ASCII upper case of `ì`. Declare a
  phrase once for each spelling that users type.
- `{field}` captures are converted with `FromStr`, and a rejected value means the
  input is not that command.
- Input that two patterns accept is declined instead of guessed. With `explicit
  ["metti {target}"]` and `explicit ["metti {target} via"]`, “metti Luna via”
  matches both and is not a command.
- The generated matcher borrows the input without allocating. Field types'
  `FromStr` implementations and custom parsers passed to `Vocabulary::new`
  can allocate.

## Build-time checks

The macro fails the build, pointing at the declaration, for each problem in this
table. It shares the [derive's capture and pattern checks](turn-macros.md#build-time-checks-1),
such as rejecting unknown captures, missing field captures, and duplicate
patterns. The `{target}` rule depends on the clause: `explicit` requires it,
and `previous` forbids it.

| Problem                                      | Message contains                          |
| -------------------------------------------- | ----------------------------------------- |
| An empty body                                | `at least one entry`                      |
| A path with fewer than two segments          | `` `Enum::Variant` ``                     |
| Entries that spell the enum path differently | `spell the enum path identically`         |
| Generic arguments on a variant               | `put generic arguments on the enum`       |
| The same variant declared twice              | `duplicate entry`                         |
| The same field declared twice in an entry    | `duplicate field`                         |
| An empty `explicit` or `previous` list       | `at least one phrase`                     |
| A clause repeated in one entry               | `` repeated `explicit` `` (or `previous`) |
| Any other clause keyword                     | `` expected `explicit` or `previous` ``   |
| An `explicit` pattern without `{target}`     | `needs a {target} capture`                |
| A `previous` phrase with `{target}`          | `cannot hold {target}`                    |
| An empty normalized `previous` phrase        | `empty phrase`                            |
| Two patterns equal ignoring ASCII case       | `duplicate pattern`                       |

## Runtime API

- `Vocabulary::new(parse)` wraps a parser function, and `Vocabulary::parse(input)`
  runs it. A vocabulary is `Copy`.
- `Vocabulary::<Action>::derived()` wraps `Action::parse_command`, including any
  `#[commands(previous = "it")]` setting on a derived action.
- `CommandHandler::new(&CATALOG, vocabulary, callback)` starts with the vocabulary
  you pass in, whether derived, declared with `vocabulary!`, or handwritten.
- `CommandHandler::set_vocabulary(vocabulary)` replaces the phrases the handler
  understands from the next turn on. The new vocabulary replaces the old one, so
  phrases of the old language are no longer recognized.
- Both methods take `impl Into<ActiveVocabulary<T>>`, so they accept a
  `Vocabulary<T>` or a `RuntimeVocabulary<T>`.

The agent keeps the remembered targets and any pending question across a switch,
because the memory belongs to the `Agent` and not to the handler. A question
asked in English can be answered after the switch.

A handwritten `TurnHandler` can offer the same: store a `Vocabulary<T>`, or an
`ActiveVocabulary<T>` to hold either kind, and call its `parse` from
`TurnHandler::parse`.

## Run-time vocabularies

A `RuntimeVocabulary<T>` owns phrases that the firmware received as strings. It
needs `T: NamedActions`, which `#[derive(TurnCommands)]` implements.

### Actions and fields

`NamedActions::ACTIONS` lists the actions that a run-time vocabulary can spell:
exactly the variants with at least one `#[command]`, in declaration order. Each
`ActionSpec` holds the variant name without `r#` and the names of its fields as
a pattern captures them. Names are compared case-sensitively, and they are the
contract with whatever sends the phrases: renaming a variant or a field breaks
the vocabularies written for the old name.

### Methods

- `RuntimeVocabulary::<Action>::new()` creates an empty vocabulary with the
  default limits.
- `RuntimeVocabulary::<Action>::with_limits(limits)` creates one with your
  `VocabularyLimits`.
- `insert_explicit(action, pattern)` adds a phrase that needs `{target}`.
- `insert_previous(action, phrase)` adds a phrase that refers to the previous
  target and has no `{target}`.
- `parse(input)` extracts the command that spells all of the input.
- `len()`, `is_empty()`, `limits()`, and `bytes()` describe what it holds.

An insertion that returns an error leaves the vocabulary unchanged. An empty
vocabulary is valid and recognizes nothing. `Clone` allocates like any other
collection and aborts when memory runs out, unlike insertion.

### Limits

`VocabularyLimits` holds `max_phrases` and `max_bytes`, which default to 256
phrases and 16 KiB (16,384 bytes). A phrase costs the byte length of the string
passed to the insertion, before normalization. The limits are checked first, so
an oversized phrase is refused without allocating. A phrase is read no further
than its ninth capture, where it is refused with `TooManyCaptures`.

`max_bytes` bounds phrase text, not heap use. Each stored phrase also costs
bookkeeping, and a short one can occupy a few hundred bytes, so `max_phrases` is
the limit that bounds the memory of a vocabulary.

### Errors

Insertion returns a `VocabularyError`, which implements `Display` and
`core::error::Error`.

| Variant            | Cause                                                         |
| ------------------ | ------------------------------------------------------------- |
| `UnknownAction`    | No action has this name                                       |
| `UnknownField`     | A `{name}` is neither a field of the action nor `{target}`    |
| `ReservedField`    | An action field is named `target` or `r#target`               |
| `DuplicateField`   | An action declares the same field name twice                  |
| `MissingField`     | A field of the action is not captured                         |
| `MissingTarget`    | An explicit phrase has no `{target}`                          |
| `TargetInPrevious` | A previous phrase has a `{target}`                            |
| `RepeatedCapture`  | A capture appears twice                                       |
| `AdjacentCaptures` | Two captures have no literal between them                     |
| `MalformedCapture` | A `{` has no matching `}`, or a capture is `{}`               |
| `UnmatchedBrace`   | A lone `}`; write `}}` for a literal brace                    |
| `EmptyPhrase`      | Nothing is left after normalization                           |
| `TooManyCaptures`  | More than eight captures, or an action with over eight fields |
| `DuplicatePattern` | Equal to an earlier phrase ignoring ASCII case                |
| `TooLarge`         | The insertion would exceed a limit                            |
| `OutOfMemory`      | A memory reservation failed                                   |

The duplicate check spans the whole vocabulary, across actions and clauses.

### Matching equivalence

A run-time vocabulary matches input like a `vocabulary!` vocabulary with the
same phrases: the same normalization, ASCII case folding, ambiguity refusal, and
`explicit` and `previous` selectors. Matching borrows the input and allocates
nothing apart from the field types' `FromStr`. Derived actions check typed
captures in pattern order and move the converted values into the first matching
action. Handwritten `NamedActions` implementations can override `accepts_field`
to reject an intermediate capture early. Its default accepts every capture and
leaves validation to `build`.
