//! Checks that firmware can build command vocabularies from strings it received at run time.

use esp_agent::{
    ActionSpec, ActiveVocabulary, Agent, Command, CommandHandler, Embedder, Knowledge, Model,
    NamedActions, RuntimeVocabulary, TargetCatalog, TargetId, TargetSelector, TurnCommands,
    TurnOutcome, Vocabulary, VocabularyError, VocabularyLimits, model, targets, vocabulary,
};
use serde_json::Value;

static TINY: Model = model!("tests/fixtures/tiny-2d");

static LAMPS: TargetCatalog = targets! {
    device Luna = 1, "Luna" {}
    device Sole = 2, "Sole" {}
    choose "lamp" from [Luna, Sole];
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Level(u8);

impl core::str::FromStr for Level {
    type Err = ();
    fn from_str(text: &str) -> Result<Self, ()> {
        let level = text.parse::<u8>().map_err(|_| ())?;
        (level <= 100).then_some(Self(level)).ok_or(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TurnCommands)]
#[commands(previous = "it")]
enum Action {
    #[command("turn on {target}", "turn {target} on")]
    TurnOn,
    #[command("turn off {target}", "turn {target} off")]
    TurnOff,
    #[command("dim {target} to {level}%")]
    SetBrightness { level: Level },
    /// Built by handwritten code.
    BeginDim,
    #[command("toggle {target}")]
    r#Type,
}

#[test]
fn named_actions_list_command_variants_in_order() {
    let names: Vec<_> = Action::ACTIONS.iter().map(|spec| spec.name).collect();
    assert_eq!(names, ["TurnOn", "TurnOff", "SetBrightness", "Type"]);
    assert_eq!(Action::ACTIONS[2].fields, ["level"]);
    assert!(Action::ACTIONS[0].fields.is_empty());
}

#[test]
#[expect(non_upper_case_globals, reason = "exercise generated parameter names")]
fn derived_named_actions_build_with_caller_constants() {
    const __action: usize = 42;
    const __fields: &[&str] = &["caller"];
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    enum Op {
        #[command("set {target} to {level}")]
        Set { level: u8 },
    }
    assert_eq!(Op::build(0, &["3"]), Some(Op::Set { level: 3 }));
    assert_eq!((__action, __fields), (42, &["caller"][..]));
}

#[test]
fn named_actions_build_from_field_text() {
    assert_eq!(
        Action::build(2, &["40"]),
        Some(Action::SetBrightness { level: Level(40) })
    );
    assert_eq!(Action::build(2, &["140"]), None);
    assert_eq!(Action::build(2, &[]), None);
    assert_eq!(Action::build(2, &["40", "40"]), None);
    assert_eq!(Action::build(0, &[]), Some(Action::TurnOn));
    assert_eq!(Action::build(0, &["40"]), None);
    assert_eq!(Action::build(3, &[]), Some(Action::r#Type));
    assert_eq!(Action::build(9, &[]), None);
}

#[test]
fn named_actions_hide_variants_without_commands() {
    assert!(!Action::ACTIONS.iter().any(|spec| spec.name == "BeginDim"));
    assert!(!Action::ACTIONS.iter().any(|spec| spec.name == "turnon"));
    assert_ne!(Action::BeginDim, Action::TurnOn);
}

#[test]
fn named_actions_take_fields_in_declaration_order_on_generic_enums() {
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    enum Preset<T>
    where
        T: core::str::FromStr,
    {
        #[command("set {target} {second} {first}")]
        Set { first: u8, second: T },
    }

    assert_eq!(Preset::<bool>::ACTIONS[0].fields, ["first", "second"]);
    assert_eq!(
        Preset::<bool>::build(0, &["7", "true"]),
        Some(Preset::Set {
            first: 7,
            second: true
        })
    );
}

/// The phrases of the compiled Italian vocabulary of `tests/vocabulary.rs`, added at run time.
fn italian() -> RuntimeVocabulary<Action> {
    let mut words = RuntimeVocabulary::new();
    words.insert_explicit("TurnOn", "accendi {target}").unwrap();
    words.insert_previous("TurnOn", "accendila").unwrap();
    words.insert_previous("TurnOn", "accendilo").unwrap();
    words.insert_explicit("TurnOff", "spegni {target}").unwrap();
    words.insert_previous("TurnOff", "spegnila").unwrap();
    words.insert_previous("TurnOff", "spegnilo").unwrap();
    words.insert_previous("TurnOff", "basta così").unwrap();
    words
        .insert_explicit("SetBrightness", "imposta {target} al {level}%")
        .unwrap();
    words
        .insert_previous("SetBrightness", "impostala al {level}%")
        .unwrap();
    words
}

#[test]
fn runtime_vocabulary_accepts_the_italian_phrases() {
    let words = italian();
    assert_eq!(words.len(), 9);
    assert!(!words.is_empty());
    assert!(RuntimeVocabulary::<Action>::new().is_empty());
}

#[test]
fn runtime_vocabulary_reports_each_invalid_phrase() {
    use VocabularyError::{
        AdjacentCaptures, EmptyPhrase, MalformedCapture, MissingField, MissingTarget,
        RepeatedCapture, TargetInPrevious, UnknownAction, UnknownField, UnmatchedBrace,
    };

    let mut words = RuntimeVocabulary::<Action>::new();
    for (action, phrase, expected) in [
        ("Accendi", "accendi {target}", UnknownAction),
        ("turnon", "accendi {target}", UnknownAction),
        ("BeginDim", "accendi {target}", UnknownAction),
        ("r#Type", "accendi {target}", UnknownAction),
        ("TurnOn", "accendi {target} {x}", UnknownField),
        ("SetBrightness", "imposta {target}", MissingField),
        ("TurnOn", "accendi", MissingTarget),
        ("TurnOn", "{target} e {target}", RepeatedCapture),
        ("SetBrightness", "{target}{level}", AdjacentCaptures),
        ("TurnOn", "accendi {target", MalformedCapture),
        ("TurnOn", "accendi {}", MalformedCapture),
        ("TurnOn", "acc}endi {target}", UnmatchedBrace),
    ] {
        assert_eq!(
            words.insert_explicit(action, phrase),
            Err(expected),
            "{action} {phrase}"
        );
    }
    assert_eq!(
        words.insert_previous("TurnOn", "accendi {target}"),
        Err(TargetInPrevious)
    );
    assert_eq!(words.insert_previous("TurnOn", " ?! "), Err(EmptyPhrase));
    assert!(words.is_empty());
}

#[test]
fn reserved_raw_field_is_only_available_to_the_derived_parser() {
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    enum Raw {
        #[command("set {target} to {r#target}")]
        Set { r#target: u8 },
    }
    assert_eq!(
        Raw::parse_command("set lamp to 3"),
        explicit(Raw::Set { r#target: 3 }, "lamp")
    );
    let mut words = RuntimeVocabulary::<Raw>::new();
    assert_eq!(
        words.insert_explicit("Set", "set {target} to {r#target}"),
        Err(VocabularyError::ReservedField)
    );
    assert!(words.is_empty());
}

/// A handwritten implementation with more fields than the derive can expose.
#[derive(Debug, PartialEq, Eq)]
struct Wide([u8; 8]);

impl NamedActions for Wide {
    const ACTIONS: &'static [ActionSpec] = &[
        ActionSpec {
            name: "Wide",
            fields: &["a", "b", "c", "d", "e", "f", "g", "h"],
        },
        ActionSpec {
            name: "Duplicate",
            fields: &["a", "a"],
        },
        ActionSpec {
            name: "RawFirst",
            fields: &["r#a", "a"],
        },
        ActionSpec {
            name: "RawLast",
            fields: &["a", "r#a"],
        },
        ActionSpec {
            name: "Repeated",
            fields: &["a", "a", "a", "a", "a", "a", "a", "a", "a"],
        },
    ];

    fn build(action: usize, fields: &[&str]) -> Option<Self> {
        let mut values = [0; 8];
        if action != 0 || fields.len() != values.len() {
            return None;
        }
        for (value, field) in values.iter_mut().zip(fields) {
            *value = field.parse().ok()?;
        }
        Some(Self(values))
    }
}

#[test]
fn runtime_vocabulary_refuses_more_than_eight_captures_or_fields() {
    let mut words = RuntimeVocabulary::<Wide>::new();
    assert_eq!(
        words.insert_explicit("Wide", "{target} {a} {b} {c} {d} {e} {f} {g} {h}"),
        Err(VocabularyError::TooManyCaptures)
    );
    assert_eq!(
        words.insert_previous("Repeated", "say {a}"),
        Err(VocabularyError::TooManyCaptures)
    );
    words
        .insert_previous("Wide", "{h} {g} {f} {e} {d} {c} {b} {a}")
        .unwrap();
    assert_eq!(
        words.parse("8 7 6 5 4 3 2 1"),
        previous(Wide([1, 2, 3, 4, 5, 6, 7, 8]))
    );
}

#[test]
fn runtime_vocabulary_rejects_duplicate_field_declarations() {
    let mut words = RuntimeVocabulary::<Wide>::new();
    let initial = "{a}/{b}/{c}/{d}/{e}/{f}/{g}/{h}";
    words.insert_previous("Wide", initial).unwrap();
    assert_eq!(
        words.insert_previous("Duplicate", "{a}"),
        Err(VocabularyError::DuplicateField)
    );
    for action in ["RawFirst", "RawLast"] {
        assert_eq!(
            words.insert_previous(action, "{a}/{r#a}"),
            Err(VocabularyError::DuplicateField),
            "{action}"
        );
    }
    assert_eq!((words.len(), words.bytes()), (1, initial.len()));
    assert_eq!(
        words.parse("1/2/3/4/5/6/7/8"),
        previous(Wide([1, 2, 3, 4, 5, 6, 7, 8]))
    );
}

#[test]
fn handwritten_validator_prunes_rejected_splits_before_build() {
    use core::sync::atomic::{AtomicUsize, Ordering};

    static BUILDS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, PartialEq, Eq)]
    struct Set(u8);

    impl NamedActions for Set {
        const ACTIONS: &'static [ActionSpec] = &[ActionSpec {
            name: "Set",
            fields: &["r#level"],
        }];

        fn accepts_field(action: usize, field: usize, text: &str) -> bool {
            action == 0 && field == 0 && text.parse::<u8>().is_ok()
        }

        fn build(action: usize, fields: &[&str]) -> Option<Self> {
            BUILDS.fetch_add(1, Ordering::Relaxed);
            match (action, fields) {
                (0, [level]) => level.parse().ok().map(Self),
                _ => None,
            }
        }
    }

    let mut words = RuntimeVocabulary::<Set>::new();
    words
        .insert_explicit("Set", "set {r#level} on {target}")
        .unwrap();
    assert_eq!(
        words.parse("set 3 on lamp on desk"),
        explicit(Set(3), "lamp on desk")
    );
    assert_eq!(BUILDS.load(Ordering::Relaxed), 1);
}

#[test]
fn runtime_vocabulary_rejects_duplicates_after_normalization() {
    let mut words = italian();
    assert_eq!(
        words.insert_previous("TurnOff", " SPEGNILA!? "),
        Err(VocabularyError::DuplicatePattern)
    );
    // The same phrase under another action.
    assert_eq!(
        words.insert_previous("TurnOn", "spegnila"),
        Err(VocabularyError::DuplicatePattern)
    );
    assert_eq!(
        words.insert_explicit("TurnOn", "Spegni {target}"),
        Err(VocabularyError::DuplicatePattern)
    );
}

#[test]
fn failed_inserts_leave_the_vocabulary_unchanged() {
    let mut words = italian();
    let (len, bytes) = (words.len(), words.bytes());
    assert!(words.insert_explicit("TurnOn", "accendi").is_err());
    assert!(words.insert_previous("TurnOn", "spegnila").is_err());
    assert_eq!((words.len(), words.bytes()), (len, bytes));
}

#[test]
fn limits_refuse_too_many_phrases_and_bytes() {
    let limits = VocabularyLimits {
        max_phrases: 1,
        max_bytes: 64,
    };
    let mut words = RuntimeVocabulary::<Action>::with_limits(limits);
    assert_eq!(words.limits(), limits);
    words.insert_previous("TurnOff", "spegnila").unwrap();
    assert_eq!(
        words.insert_previous("TurnOn", "accendila"),
        Err(VocabularyError::TooLarge)
    );
    // The limits are checked before anything else about the phrase.
    assert_eq!(
        words.insert_previous("Nope", "{"),
        Err(VocabularyError::TooLarge)
    );

    let mut words = RuntimeVocabulary::<Action>::with_limits(VocabularyLimits {
        max_phrases: 8,
        max_bytes: 12,
    });
    words.insert_previous("TurnOff", "spegnila").unwrap(); // 8 bytes
    assert_eq!(
        words.insert_previous("TurnOn", "accendila"), // 17 > 12
        Err(VocabularyError::TooLarge)
    );
    words.insert_previous("TurnOn", "vai!").unwrap(); // 12, still fits
    assert_eq!(words.bytes(), 12);
}

#[test]
fn default_limits_hold_a_device_language() {
    let limits = VocabularyLimits::default();
    assert_eq!((limits.max_phrases, limits.max_bytes), (256, 16 * 1024));
    assert_eq!(RuntimeVocabulary::<Action>::new().limits(), limits);
    assert_eq!(RuntimeVocabulary::<Action>::default().limits(), limits);
}

#[test]
fn a_cloned_vocabulary_grows_on_its_own() {
    let words = italian();
    let mut copy = words.clone();
    copy.insert_previous("TurnOn", "vai").unwrap();
    assert_eq!((words.len(), copy.len()), (9, 10));
    assert!(format!("{words:?}").starts_with("RuntimeVocabulary"));
}

#[test]
fn runtime_vocabularies_are_send_and_sync() {
    fn shared<T: Send + Sync>() {}
    // The vocabulary stores no action, so the action type does not matter.
    shared::<RuntimeVocabulary<std::rc::Rc<u8>>>();
}

#[test]
fn vocabulary_errors_display_a_sentence() {
    let error: &dyn core::error::Error = &VocabularyError::TooLarge;
    assert_eq!(
        error.to_string(),
        "the phrase exceeds the limits of the vocabulary"
    );
    assert_eq!(
        VocabularyError::MissingTarget.to_string(),
        "the phrase needs a {target} capture"
    );
}

include!("support/italian_vocabulary.rs");

const PARITY_INPUTS: &[&str] = &[
    "  SPEGNILA! ",
    "accendi  Luna .",
    "Basta così!",
    "Basta così!\u{2003}?",
    "BASTA COSÌ",
    "spegnila adesso",
    "accendila Luna",
    "spegni",
    "",
    " .!? ",
    "accendi it",
    "accendi Luna! ?",
    "Imposta Luna al 40%",
    "impostala al 40%",
    "impostala al 140%",
    "imposta Luna al tanto%",
    "imposta la luce al piano al 40%",
    "imposta  al 40%",
    "SPEGNI Lüster",
    "accendì Luna",
];

fn previous<T>(action: T) -> Option<Command<'static, T>> {
    Some(Command {
        action,
        target: TargetSelector::Previous,
    })
}

fn explicit<T>(action: T, target: &str) -> Option<Command<'_, T>> {
    Some(Command {
        action,
        target: TargetSelector::Explicit(target),
    })
}

#[test]
fn runtime_vocabulary_matches_like_the_compiled_one() {
    let runtime = italian();
    for line in PARITY_INPUTS {
        assert_eq!(runtime.parse(line), ITALIAN.parse(line), "{line}");
    }
    assert_eq!(
        runtime.parse("accendi  Luna ."),
        explicit(Action::TurnOn, "Luna")
    );
    assert_eq!(runtime.parse("spegnila"), previous(Action::TurnOff));
}

/// An action whose fields can reject a split that another split accepts.
#[derive(Debug, PartialEq, Eq, TurnCommands)]
enum Rename {
    #[command("rename {target} {first} {second}")]
    Rename { first: String, second: u8 },
    #[command("toggle {target} {enabled}")]
    Toggle { enabled: bool },
    #[command("count {target} {level}")]
    Count { level: u8 },
}

#[test]
fn runtime_vocabulary_converts_an_intermediate_field_once() {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, PartialEq, Eq)]
    struct Ticket;
    impl core::str::FromStr for Ticket {
        type Err = ();
        fn from_str(text: &str) -> Result<Self, ()> {
            (CALLS.fetch_add(1, Ordering::Relaxed) == 0 && text == "xyz")
                .then_some(Self)
                .ok_or(())
        }
    }
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    enum Op {
        #[command("set {code} on {target}")]
        Set { code: Ticket },
    }
    let compiled = vocabulary! {
        Op::Set { code: Ticket } => explicit ["set {code} on {target}"]
    };
    let mut runtime = RuntimeVocabulary::<Op>::new();
    runtime
        .insert_explicit("Set", "set {code} on {target}")
        .unwrap();
    let input = "set xyz on Luna";
    let expected = compiled.parse(input);
    assert_eq!(expected, explicit(Op::Set { code: Ticket }, "Luna"));
    assert_eq!(CALLS.swap(0, Ordering::Relaxed), 1);
    assert_eq!(runtime.parse(input), expected);
    assert_eq!(CALLS.load(Ordering::Relaxed), 1);
}

#[test]
fn runtime_vocabulary_resolves_splits_like_the_compiled_one() {
    let compiled = vocabulary! {
        Rename::Rename { first: String, second: u8 } =>
            explicit ["chiama {target}: {second} poi {first}"],
            previous ["rename it {first} / {second}", "{first} → {second}"];
        Rename::Toggle { enabled: bool } => previous ["impostala a {enabled}"];
        Rename::Count { level: u8 } =>
            previous ["impostala a {level}"],
            explicit ["metti {target} a {level}"];
    };
    let mut runtime = RuntimeVocabulary::<Rename>::new();
    runtime
        .insert_explicit("Rename", "chiama {target}: {second} poi {first}")
        .unwrap();
    runtime
        .insert_previous("Rename", "rename it {first} / {second}")
        .unwrap();
    runtime
        .insert_previous("Rename", "{first} → {second}")
        .unwrap();
    runtime
        .insert_previous("Toggle", "impostala a {enabled}")
        .unwrap();
    runtime
        .insert_previous("Count", "impostala a {level}")
        .unwrap();
    runtime
        .insert_explicit("Count", "metti {target} a {level}")
        .unwrap();
    for line in [
        "rename it a / b / 7",
        "rename it a / 7",
        "rename it a / b",
        "chiama Luna: 7 poi Sole",
        "chiama il: faro: 7 poi Sole poi basta",
        "Lüster→7",
        "a → b → 7",
        "impostala a 7",
        "impostala a true",
        "impostala a forse",
        "metti Luna a 7",
        "metti la lampada a muro a 7",
    ] {
        assert_eq!(runtime.parse(line), compiled.parse(line), "{line}");
    }
}

#[test]
fn documented_german_commands_select_explicit_and_previous_targets() {
    let mut german = RuntimeVocabulary::<Action>::new();
    german
        .insert_explicit("TurnOff", "schalte {target} aus")
        .unwrap();
    let follow_up = "ausschalten";
    german.insert_previous("TurnOff", follow_up).unwrap();

    assert_eq!(
        german.parse("schalte Luna aus"),
        explicit(Action::TurnOff, "Luna")
    );
    assert_eq!(german.parse(follow_up), previous(Action::TurnOff));
}

#[test]
fn runtime_vocabulary_declines_ambiguous_input() {
    let mut words = RuntimeVocabulary::<Action>::new();
    words.insert_explicit("TurnOn", "metti {target}").unwrap();
    words
        .insert_explicit("TurnOff", "metti {target} via")
        .unwrap();
    words.insert_previous("TurnOff", "metti subito").unwrap();
    assert_eq!(words.parse("metti Luna via"), None);
    assert_eq!(words.parse("metti subito"), None);
    assert_eq!(words.parse("metti Luna"), explicit(Action::TurnOn, "Luna"));
}

#[test]
fn runtime_vocabulary_handles_non_ascii_text() {
    let mut words = RuntimeVocabulary::<Action>::new();
    words
        .insert_explicit("TurnOff", "schalte {target} aus")
        .unwrap();
    words.insert_previous("TurnOff", "basta così").unwrap();
    words
        .insert_explicit("SetBrightness", "dimme {target}→{level}%")
        .unwrap();
    assert_eq!(
        words.parse("Schalte Lüster aus"),
        explicit(Action::TurnOff, "Lüster")
    );
    assert_eq!(words.parse("Basta così!"), previous(Action::TurnOff));
    assert_eq!(words.parse("BASTA COSÌ"), None);
    assert_eq!(
        words.parse("dimme Lüster→40%"),
        explicit(Action::SetBrightness { level: Level(40) }, "Lüster")
    );
    for line in ["ü", "schalte ü", "ü aus", "dimme ü%"] {
        assert_eq!(words.parse(line), None, "{line}");
    }
}

#[test]
fn an_empty_runtime_vocabulary_recognizes_nothing() {
    let words = RuntimeVocabulary::<Action>::new();
    assert_eq!(words.parse("spegnila"), None);
    assert_eq!(words.parse(""), None);
}

/// What a coordinator sends to a device that speaks Italian.
const PAYLOAD: &str = r#"{"language":"it","phrases":[
    {"action":"TurnOn","explicit":["accendi {target}"],"previous":["accendila"]},
    {"action":"TurnOff","explicit":["spegni {target}"],"previous":["spegnila"]}]}"#;

// ANCHOR: coordinator
/// Builds the whole language of a payload, or reports the first problem of it.
fn vocabulary_from(
    payload: &str,
) -> Result<RuntimeVocabulary<Action>, Box<dyn core::error::Error>> {
    let payload: Value = serde_json::from_str(payload)?;
    let mut words = RuntimeVocabulary::new();
    let entries = payload["phrases"].as_array().ok_or("no phrases")?;
    for entry in entries {
        let action = entry["action"].as_str().ok_or("no action name")?;
        for phrase in phrases(entry, "explicit")? {
            words.insert_explicit(action, phrase?)?;
        }
        for phrase in phrases(entry, "previous")? {
            words.insert_previous(action, phrase?)?;
        }
    }
    Ok(words)
}

/// Reads one optional list of phrases, refusing anything that is not a string.
fn phrases<'a>(
    entry: &'a Value,
    clause: &str,
) -> Result<impl Iterator<Item = Result<&'a str, &'static str>>, &'static str> {
    let list = match entry.get(clause) {
        Some(list) => list.as_array().ok_or("a clause is not a list")?.as_slice(),
        None => &[],
    };
    Ok(list
        .iter()
        .map(|phrase| phrase.as_str().ok_or("a phrase is not a string")))
}
// ANCHOR_END: coordinator

type Done = (Action, Vec<TargetId>);

fn run(action: Action, ids: &[TargetId]) -> Result<Done, ()> {
    Ok((action, ids.to_vec()))
}

fn done(action: Action, ids: &[TargetId]) -> TurnOutcome<Done, ()> {
    TurnOutcome::Executed((action, ids.to_vec()))
}

// ANCHOR: runtime_switch
/// Installs the new language once every phrase in the payload has been accepted.
fn install_language<F>(
    handler: &mut CommandHandler<'_, Action, F>,
    payload: &str,
) -> Result<(), Box<dyn core::error::Error>> {
    handler.set_vocabulary(vocabulary_from(payload)?);
    Ok(())
}

#[test]
fn handler_switches_to_a_runtime_vocabulary_and_keeps_the_target() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::<Action>::derived(), run);
    assert_eq!(
        agent.turn("Turn on Luna", &mut handler),
        done(Action::TurnOn, &[1])
    );

    install_language(&mut handler, PAYLOAD).unwrap();
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        done(Action::TurnOff, &[1])
    );
    assert_eq!(
        agent.turn("Turn off Luna", &mut handler),
        TurnOutcome::Unrecognized
    );

    handler.set_vocabulary(Vocabulary::derived());
    assert_eq!(
        agent.turn("Turn on it", &mut handler),
        done(Action::TurnOn, &[1])
    );
}
// ANCHOR_END: runtime_switch

#[test]
fn a_pending_question_survives_installing_a_runtime_vocabulary() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::derived(), run);
    assert_eq!(
        agent.turn("Turn on the lamp", &mut handler),
        TurnOutcome::Clarify(vec![1, 2])
    );
    handler.set_vocabulary(vocabulary_from(PAYLOAD).unwrap());
    assert_eq!(agent.turn("Luna", &mut handler), done(Action::TurnOn, &[1]));
}

#[test]
fn a_rejected_payload_keeps_the_current_language() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::<Action>::derived(), run);

    install_language(&mut handler, PAYLOAD).unwrap();
    assert_eq!(
        agent.turn("Accendi Luna", &mut handler),
        done(Action::TurnOn, &[1])
    );

    let error = install_language(
        &mut handler,
        r#"{"phrases":[{"action":"TurnOn","explicit":["light {target}", "light"]}]}"#,
    )
    .expect_err("the second explicit phrase has no target");
    assert_eq!(
        error.downcast_ref::<VocabularyError>(),
        Some(&VocabularyError::MissingTarget)
    );
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        done(Action::TurnOff, &[1])
    );
    assert_eq!(
        agent.turn("light Sole", &mut handler),
        TurnOutcome::Unrecognized
    );

    let error = install_language(&mut handler, "not json").unwrap_err();
    assert!(
        error
            .downcast_ref::<serde_json::Error>()
            .unwrap()
            .is_syntax()
    );
    for (payload, expected) in [
        (r#"{}"#, "no phrases"),
        (
            r#"{"phrases":[{"explicit":["accendi {target}"]}]}"#,
            "no action name",
        ),
        (
            r#"{"phrases":[{"action":"TurnOn","explicit":"accendi {target}"}]}"#,
            "a clause is not a list",
        ),
        (
            r#"{"phrases":[{"action":"TurnOn","previous":[7]}]}"#,
            "a phrase is not a string",
        ),
    ] {
        assert_eq!(
            install_language(&mut handler, payload)
                .unwrap_err()
                .to_string(),
            expected,
            "{payload}"
        );
    }
}

#[test]
fn a_handler_can_start_with_a_runtime_vocabulary() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, vocabulary_from(PAYLOAD).unwrap(), run);
    assert!(format!("{handler:?}").contains("RuntimeVocabulary"));
    assert_eq!(
        agent.turn("Accendi Sole", &mut handler),
        done(Action::TurnOn, &[2])
    );
}

#[test]
fn an_active_vocabulary_parses_with_either_kind() {
    let compiled = ActiveVocabulary::from(ITALIAN);
    let runtime = ActiveVocabulary::from(italian());
    for words in [compiled, runtime] {
        assert_eq!(words.parse("spegnila"), previous(Action::TurnOff));
        assert!(format!("{words:?}").starts_with("ActiveVocabulary"));
    }
}

#[test]
fn compiled_vocabularies_need_no_named_actions() {
    #[derive(Debug, PartialEq, Eq)]
    enum Plain {
        Off,
    }

    let words = vocabulary! { Plain::Off => previous ["spegnila"] };
    let mut handler =
        CommandHandler::new(&LAMPS, words, |_: Plain, _: &[TargetId]| Ok::<_, ()>(()));
    handler.set_vocabulary(words);
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::NeedTarget
    );
}
