//! Checks that firmware can switch the command vocabulary during a conversation.

use esp_agent::{
    Agent, Command, CommandHandler, DynamicTarget, DynamicTargetCatalog, Embedder, Knowledge,
    Model, TargetCatalog, TargetId, TargetLookup, TargetSelector, TurnCommands, TurnOutcome,
    Vocabulary, model, targets, vocabulary,
};

static TINY: Model = model!("tests/fixtures/tiny-2d");

// ANCHOR: catalog
static LAMPS: TargetCatalog = targets! {
    device Luna = 1, "Luna" {}
    device Sole = 2, "Sole" {}
    choose "lamp" from [Luna, Sole];
    group ["living room", "salotto"] = [Luna, Sole];
};
// ANCHOR_END: catalog

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Level(u8);

impl core::str::FromStr for Level {
    type Err = ();
    fn from_str(text: &str) -> Result<Self, ()> {
        let level = text.parse::<u8>().map_err(|_| ())?;
        (level <= 100).then_some(Self(level)).ok_or(())
    }
}

// ANCHOR: actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, TurnCommands)]
#[commands(previous = "it")]
enum Action {
    #[command("turn on {target}", "turn {target} on")]
    TurnOn,
    #[command("turn off {target}", "turn {target} off")]
    TurnOff,
    #[command("dim {target} to {level}%")]
    SetBrightness { level: Level },
}
// ANCHOR_END: actions

include!("support/italian_vocabulary.rs");

fn previous(action: Action) -> Option<Command<'static, Action>> {
    Some(Command {
        action,
        target: TargetSelector::Previous,
    })
}

fn explicit(action: Action, target: &str) -> Option<Command<'_, Action>> {
    Some(Command {
        action,
        target: TargetSelector::Explicit(target),
    })
}

#[test]
fn vocabulary_phrases_match_the_whole_normalized_input() {
    for (line, expected) in [
        ("  SPEGNILA! ", previous(Action::TurnOff)),
        ("accendi  Luna .", explicit(Action::TurnOn, "Luna")),
        ("Basta così!", previous(Action::TurnOff)),
        ("Basta così!\u{2003}?", previous(Action::TurnOff)),
        // Literals fold ASCII case only, so an upper-case Ì is a different letter.
        ("BASTA COSÌ", None),
        ("spegnila adesso", None),
        ("accendila Luna", None),
        ("spegni", None),
        ("", None),
        (" .!? ", None),
    ] {
        assert_eq!(ITALIAN.parse(line), expected, "{line}");
    }
}

#[test]
fn vocabulary_captures_legacy_previous_word_as_an_explicit_target() {
    assert_eq!(ITALIAN.parse("accendi it"), explicit(Action::TurnOn, "it"));
    assert_eq!(
        Action::parse_command("turn on it"),
        previous(Action::TurnOn)
    );
    assert_eq!(
        Action::parse_command("turn on it! ?"),
        previous(Action::TurnOn)
    );
    assert_eq!(
        Action::parse_command("turn on Luna! ?"),
        explicit(Action::TurnOn, "Luna!")
    );
    assert_eq!(
        ITALIAN.parse("accendi Luna! ?"),
        explicit(Action::TurnOn, "Luna")
    );
}

#[test]
fn vocabulary_fields_convert_typed_values() {
    let dim = Action::SetBrightness { level: Level(40) };
    assert_eq!(ITALIAN.parse("Imposta Luna al 40%"), explicit(dim, "Luna"));
    assert_eq!(ITALIAN.parse("impostala al 40%"), previous(dim));
    assert_eq!(ITALIAN.parse("impostala al 140%"), None);
    assert_eq!(ITALIAN.parse("imposta Luna al tanto%"), None);
}

#[test]
fn vocabulary_converts_fields_by_name_not_capture_position() {
    #[derive(Debug, PartialEq, Eq)]
    enum Preset {
        Set { level: u8, enabled: bool },
    }

    let words = vocabulary! {
        Preset::Set { level: u8, enabled: bool } =>
            explicit ["imposta {target} a {enabled} con livello {level}"],
            previous ["impostala a {enabled} con livello {level}"];
    };
    for (input, target) in [
        (
            "imposta Luna a true con livello 42",
            TargetSelector::Explicit("Luna"),
        ),
        ("impostala a true con livello 42", TargetSelector::Previous),
    ] {
        assert_eq!(
            words.parse(input),
            Some(Command {
                action: Preset::Set {
                    level: 42,
                    enabled: true
                },
                target,
            }),
            "{input}"
        );
    }
}

#[test]
fn vocabulary_factory_uses_an_enclosing_type_parameter() {
    #[derive(Debug, PartialEq, Eq)]
    enum Dim<T> {
        Set { level: T },
    }

    fn italian<T: core::str::FromStr>() -> Vocabulary<Dim<T>> {
        vocabulary! {
            Dim::<T>::Set { level: T } => previous ["impostalo a {level}"];
        }
    }

    assert_eq!(
        italian::<u8>().parse("impostalo a 42"),
        Some(Command {
            action: Dim::Set { level: 42 },
            target: TargetSelector::Previous,
        })
    );
}

#[test]
fn previous_phrase_accepts_a_capture_as_the_entire_input() {
    #[derive(Debug, PartialEq, Eq)]
    enum Dim {
        Set { level: u8 },
    }

    static BARE_LEVEL: Vocabulary<Dim> = vocabulary! {
        Dim::Set { level: u8 } => previous ["{level}"];
    };
    assert_eq!(
        BARE_LEVEL.parse(" 255! "),
        Some(Command {
            action: Dim::Set { level: 255 },
            target: TargetSelector::Previous,
        })
    );
    assert_eq!(BARE_LEVEL.parse("256"), None);
    assert_eq!(BARE_LEVEL.parse(""), None);
}

#[test]
#[expect(non_upper_case_globals, reason = "match the generated binding names")]
fn vocabulary_bindings_do_not_resolve_to_caller_constants() {
    const __input: u8 = 4;
    const __found: u8 = 1;
    const __target: &str = "collision";
    const __index: usize = 0;
    const __part: &str = "collision";
    const __field_0: u8 = 2;
    const __esp_agent_vocabulary: u8 = 3;

    #[derive(Debug, PartialEq, Eq)]
    enum Op<const N: u8> {
        Off,
        Dim { level: u8 },
    }

    let words = vocabulary! {
        Op::<1>::Off => previous ["spegnila"];
        Op::<1>::Dim { level: u8 } => explicit ["imposta {target} al {level}%"];
    };
    assert_eq!(
        words.parse("spegnila").unwrap().target,
        TargetSelector::Previous
    );
    assert_eq!(
        words.parse("imposta Luna al 40%"),
        Some(Command {
            action: Op::Dim { level: 40 },
            target: TargetSelector::Explicit("Luna"),
        })
    );
    let _ = (__target, __index, __part, __field_0);

    let words: Vocabulary<Op<4>> = vocabulary! { Op::<__input>::Off => previous ["spegnila"] };
    assert_eq!(words.parse("spegnila").unwrap().action, Op::Off);
    let words: Vocabulary<Op<1>> = vocabulary! { Op::<__found>::Off => previous ["spegnila"] };
    assert_eq!(words.parse("spegnila").unwrap().action, Op::Off);
    let words: Vocabulary<Op<3>> = vocabulary! {
        Op::<__esp_agent_vocabulary>::Off => previous ["spegnila"]
    };
    assert_eq!(words.parse("spegnila").unwrap().action, Op::Off);
}

#[test]
#[expect(non_upper_case_globals, reason = "match generated binding names")]
fn vocabulary_resolves_constants_emitted_by_nested_macros() {
    const __found: usize = 3;
    const __field_0: usize = 3;
    macro_rules! setting {
        () => {
            __found
        };
    }
    macro_rules! field_setting {
        () => {
            __field_0
        };
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Level<const N: usize>(u8);

    impl<const N: usize> core::str::FromStr for Level<N> {
        type Err = core::num::ParseIntError;

        fn from_str(text: &str) -> Result<Self, Self::Err> {
            text.parse().map(Self)
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Op<const N: usize> {
        Off,
        Dim { level: Level<N> },
    }

    static WORDS: Vocabulary<Op<3>> = vocabulary! {
        Op::<{ setting!() }>::Off => previous ["spegnila"];
        Op::<{ setting!() }>::Dim { level: Level<{ field_setting!() }> } =>
            previous ["impostala a {level}"];
    };
    assert_eq!(WORDS.parse("spegnila").unwrap().action, Op::Off);
    assert_eq!(
        WORDS.parse("impostala a 42").unwrap().action,
        Op::Dim { level: Level(42) }
    );
}

#[test]
fn vocabulary_accepts_an_associated_type_projection_alias() {
    #[derive(Debug, PartialEq, Eq)]
    enum Op {
        Dim { level: u8 },
    }

    trait ActionType {
        type Action;
    }

    struct Key<const N: usize>;

    impl ActionType for Key<3> {
        type Action = Op;
    }

    type Selected<const N: usize> = <Key<N> as ActionType>::Action;

    static WORDS: Vocabulary<Selected<3>> = vocabulary! {
        Selected::<3>::Dim { level: u8 } => previous ["impostala a {level}"];
    };
    assert_eq!(
        WORDS.parse("impostala a 42").unwrap().action,
        Op::Dim { level: 42 }
    );
}

#[test]
fn previous_phrase_requires_a_unique_valid_split() {
    #[derive(Debug, PartialEq, Eq)]
    enum Names<T> {
        Rename { first: String, second: T },
    }

    static OWNED: Vocabulary<Names<String>> = vocabulary! {
        Names::<String>::Rename { first: String, second: String } => previous ["rename it {first} / {second}"]
    };
    let vocabulary = OWNED;
    assert_eq!(vocabulary.parse("rename it a / b / c"), None);
    assert_eq!(
        vocabulary.parse("rename it a / b"),
        Some(Command {
            action: Names::Rename {
                first: String::from("a"),
                second: String::from("b"),
            },
            target: TargetSelector::Previous,
        })
    );

    let typed = vocabulary! {
        Names::<u8>::Rename { first: String, second: u8 } => previous ["rename it {first} / {second}"]
    };
    assert_eq!(
        typed.parse("rename it a / b / 7"),
        Some(Command {
            action: Names::Rename {
                first: String::from("a / b"),
                second: 7,
            },
            target: TargetSelector::Previous,
        })
    );
}

#[test]
fn previous_phrase_can_parse_eight_typed_fields() {
    #[derive(Debug, PartialEq, Eq)]
    enum Preset {
        Set {
            a: u8,
            b: u8,
            c: u8,
            d: u8,
            e: u8,
            f: u8,
            g: u8,
            h: u8,
        },
    }

    let vocabulary = vocabulary! {
        Preset::Set { a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8 } =>
            previous ["{a} {b} {c} {d} {e} {f} {g} {h}"];
    };
    assert_eq!(
        vocabulary.parse("1 2 3 4 5 6 7 8"),
        Some(Command {
            action: Preset::Set {
                a: 1,
                b: 2,
                c: 3,
                d: 4,
                e: 5,
                f: 6,
                g: 7,
                h: 8
            },
            target: TargetSelector::Previous,
        })
    );
}

#[test]
fn vocabulary_declines_input_two_entries_accept() {
    static OVERLAP: Vocabulary<Action> = vocabulary! {
        Action::TurnOn => explicit ["metti {target}"];
        Action::TurnOff => previous ["mettila via", "metti subito"], explicit ["metti {target} via"];
    };
    static PREVIOUS_FIRST: Vocabulary<Action> = vocabulary! {
        Action::TurnOff => previous ["mettila via", "metti subito"], explicit ["metti {target} via"];
        Action::TurnOn => explicit ["metti {target}"];
    };
    assert_eq!(OVERLAP.parse("metti Luna via"), None);
    assert_eq!(PREVIOUS_FIRST.parse("metti Luna via"), None);
    assert_eq!(OVERLAP.parse("metti subito"), None);
    assert_eq!(PREVIOUS_FIRST.parse("metti subito"), None);
    assert_eq!(
        OVERLAP.parse("metti Luna"),
        explicit(Action::TurnOn, "Luna")
    );
    assert_eq!(OVERLAP.parse("mettila via"), previous(Action::TurnOff));
}

#[test]
fn vocabulary_only_counts_overlapping_entries_with_valid_fields() {
    #[derive(Debug, PartialEq, Eq)]
    enum Setting {
        SetLevel { level: u8 },
        SetEnabled { enabled: bool },
    }

    let numeric_first = vocabulary! {
        Setting::SetLevel { level: u8 } => previous ["impostala a {level}"];
        Setting::SetEnabled { enabled: bool } => previous ["impostala a {enabled}"];
    };
    let boolean_first = vocabulary! {
        Setting::SetEnabled { enabled: bool } => previous ["impostala a {enabled}"];
        Setting::SetLevel { level: u8 } => previous ["impostala a {level}"];
    };
    for words in [numeric_first, boolean_first] {
        assert_eq!(
            words.parse("impostala a 7"),
            Some(Command {
                action: Setting::SetLevel { level: 7 },
                target: TargetSelector::Previous,
            })
        );
        assert_eq!(
            words.parse("impostala a true"),
            Some(Command {
                action: Setting::SetEnabled { enabled: true },
                target: TargetSelector::Previous,
            })
        );
    }
}

type Done = (Action, Vec<TargetId>);

fn run(action: Action, ids: &[TargetId]) -> Result<Done, ()> {
    Ok((action, ids.to_vec()))
}

fn done(action: Action, ids: &[TargetId]) -> TurnOutcome<Done, ()> {
    TurnOutcome::Executed((action, ids.to_vec()))
}

// ANCHOR: switch
#[test]
fn italian_follow_ups_act_on_the_lamp_an_english_command_selected() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::derived(), run);
    assert_eq!(
        agent.turn("Turn on Luna", &mut handler),
        done(Action::TurnOn, &[1])
    );

    handler.set_vocabulary(ITALIAN);
    assert_eq!(
        agent.turn("Spegnila! ?", &mut handler),
        done(Action::TurnOff, &[1])
    );
    assert_eq!(
        agent.turn("Accendila", &mut handler),
        done(Action::TurnOn, &[1])
    );
    assert_eq!(
        agent.turn("Turn off Luna", &mut handler),
        TurnOutcome::Unrecognized
    );
}
// ANCHOR_END: switch

#[test]
fn italian_follow_up_without_memory_needs_a_target() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, ITALIAN, run);
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::NeedTarget
    );
}

#[test]
fn italian_follow_up_after_a_group_asks_which_lamp() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::derived(), run);
    assert_eq!(
        agent.turn("Turn on the living room", &mut handler),
        done(Action::TurnOn, &[1, 2])
    );
    handler.set_vocabulary(ITALIAN);
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::Clarify(vec![1, 2])
    );
    assert_eq!(
        agent.turn("Luna", &mut handler),
        done(Action::TurnOff, &[1])
    );
}

#[test]
fn italian_follow_up_only_offers_available_group_members() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut lamps = DynamicTargetCatalog::new();
    lamps.insert(DynamicTarget::new(1, "Luna")).unwrap();
    lamps.insert(DynamicTarget::new(2, "Sole")).unwrap();
    lamps.insert_group(["living room"], [1, 2]).unwrap();
    assert_eq!(
        agent.turn(
            "Turn on the living room",
            &mut CommandHandler::new(&lamps, Vocabulary::derived(), run),
        ),
        done(Action::TurnOn, &[1, 2])
    );

    lamps.remove(2).unwrap();
    let catalog: &dyn TargetLookup = &lamps;
    let mut handler = CommandHandler::new(catalog, Vocabulary::derived(), run);
    handler.set_vocabulary(ITALIAN);
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::Clarify(vec![1])
    );
    assert_eq!(
        agent.turn("Luna", &mut handler),
        done(Action::TurnOff, &[1])
    );
}

#[test]
fn italian_follow_up_keeps_the_last_successful_target_after_failure() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler =
        CommandHandler::new(&LAMPS, Vocabulary::<Action>::derived(), |action, ids| {
            if ids == [2] {
                Err("offline")
            } else {
                Ok((action, ids.to_vec()))
            }
        });
    assert_eq!(
        agent.turn("Turn on Luna", &mut handler),
        TurnOutcome::Executed((Action::TurnOn, vec![1]))
    );
    handler.set_vocabulary(ITALIAN);
    assert_eq!(
        agent.turn("Accendi Sole", &mut handler),
        TurnOutcome::Failed("offline")
    );
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::Executed((Action::TurnOff, vec![1]))
    );
}

#[test]
fn italian_follow_up_cannot_reuse_a_group_that_does_not_fit_in_memory() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    for limit in [0, 1] {
        let mut agent = Agent::builder(TINY, &knowledge)
            .context(limit)
            .build()
            .unwrap();
        let mut handler = CommandHandler::new(&LAMPS, Vocabulary::derived(), run);
        assert_eq!(
            agent.turn("Turn on the living room", &mut handler),
            done(Action::TurnOn, &[1, 2])
        );
        handler.set_vocabulary(ITALIAN);
        assert_eq!(
            agent.turn("Spegnila", &mut handler),
            TurnOutcome::NeedTarget,
            "context limit {limit}"
        );
    }
}

#[test]
fn pending_question_survives_a_language_switch() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, Vocabulary::derived(), run);
    assert_eq!(
        agent.turn("Turn on the lamp", &mut handler),
        TurnOutcome::Clarify(vec![1, 2])
    );
    handler.set_vocabulary(ITALIAN);
    assert_eq!(agent.turn("Sole", &mut handler), done(Action::TurnOn, &[2]));
}

#[test]
fn switching_back_restores_the_derived_commands() {
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut handler = CommandHandler::new(&LAMPS, ITALIAN, run);
    assert_eq!(
        agent.turn("Accendi Luna", &mut handler),
        done(Action::TurnOn, &[1])
    );
    handler.set_vocabulary(Vocabulary::derived());
    assert_eq!(
        agent.turn("Spegnila", &mut handler),
        TurnOutcome::Unrecognized
    );
    assert_eq!(
        agent.turn("Turn it off", &mut handler),
        done(Action::TurnOff, &[1])
    );
}
