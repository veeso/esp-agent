//! Exercises the declarative catalog and command macros the way firmware uses them.

use esp_agent::{
    CatalogError, DynamicTarget, DynamicTargetCatalog, TargetCatalog, TargetLookup, TargetMatch,
    target, targets,
};

static LIGHTS: TargetCatalog = targets! {
    device Desk = 1, "desk lamp" { replies ["near", "desk one"]; }
    device Floor = 2, "floor lamp" { replies ["near"]; }
    choose "lamp" from [Desk, Floor];
    group ["room", "room lights"] = [Desk, Floor];
};

#[test]
fn catalog_distinguishes_commands_from_replies() {
    use TargetMatch::{Ambiguous, Missing, Resolved};
    for (input, expected) in [
        (" THE DESK LAMP?! ", Resolved(vec![1])),
        ("lamp", Ambiguous(vec![1, 2])),
        ("the ROOM lights", Resolved(vec![1, 2])),
        ("desk one", Missing),
    ] {
        assert_eq!(LIGHTS.resolve_targets(input), expected);
    }
    assert!(LIGHTS.matches_target(1, "the desk lamp"));
    assert!(LIGHTS.matches_target(1, "desk one"));
    assert!(!LIGHTS.matches_target(2, "desk one"));
    assert!([1, 2].iter().all(|&id| LIGHTS.matches_target(id, "near")));
    assert_eq!(LIGHTS.name(1), Some("desk lamp"));
    assert_eq!(LIGHTS.name(3), None);
}

use esp_agent::{Command, CommandHandler, TargetSelector, TurnCommands, TurnHandler, Vocabulary};

#[derive(Debug, PartialEq, Eq)]
struct Level(u8);

impl core::str::FromStr for Level {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let n = s.parse::<u8>().map_err(|_| ())?;
        (n <= 100).then_some(Self(n)).ok_or(())
    }
}

#[derive(Debug, PartialEq, Eq, TurnCommands)]
#[commands(previous = "that")]
enum Op {
    #[command("dim {target} to {level}%", "dim {target} to {level} percent")]
    Dim {
        level: Level,
    },
    #[command("select {target}")]
    Select,
    #[command("select the {target}")]
    SelectNamed,
    Handwritten,
}

#[test]
fn commands_extract_validated_values_from_whole_input() {
    let _ = Op::Handwritten;
    for (line, target, n) in [
        ("DIM that to 0%!", TargetSelector::Previous, 0),
        ("dim it to 10%", TargetSelector::Explicit("it"), 10),
        (
            "dim entrée to hall to 100 percent",
            TargetSelector::Explicit("entrée to hall"),
            100,
        ),
    ] {
        assert_eq!(
            Op::parse_command(line),
            Some(Command {
                action: Op::Dim { level: Level(n) },
                target,
            })
        );
    }
    for line in [
        "dim  to 2%",
        "dim it to 101%",
        "dim it to 2% later",
        "select the desk",
        "Handwritten",
    ] {
        assert_eq!(Op::parse_command(line), None, "{line}");
    }
}

#[test]
fn adapter_preserves_callback_errors_and_checks_input_despite_hints() {
    let mut adapter = Op::handler(&LIGHTS, |_, ids: &[u32]| {
        assert_eq!(ids, &[2]);
        Err::<(), _>("offline")
    });
    assert!(adapter.parse("unrelated", Some(&Op::Select)).is_none());
    assert_eq!(
        adapter.resolve_targets("floor lamp"),
        TargetMatch::Resolved(vec![2])
    );
    assert!(adapter.matches_target(2, "near"));
    assert_eq!(adapter.dispatch(Op::Select, &[2]), Err("offline"));
}

#[test]
fn adapter_accepts_concrete_and_erased_catalog_references() {
    let catalog = LIGHTS;
    let reference = &catalog;
    let adapter = Op::handler::<_, (), ()>(&reference, |_, _| Ok(()));
    assert_eq!(
        adapter.resolve_targets("desk lamp"),
        TargetMatch::Resolved(vec![1])
    );
    assert_eq!(
        Op::handler(&&catalog, |_, _| Ok::<(), ()>(())).resolve_targets("floor lamp"),
        TargetMatch::Resolved(vec![2])
    );
    let lookup: &dyn TargetLookup = &catalog;
    assert_eq!(
        Op::handler(lookup, |_, _| Ok::<(), ()>(())).resolve_targets("desk lamp"),
        TargetMatch::Resolved(vec![1])
    );
}

#[test]
fn derived_commands_do_not_shadow_a_firmware_vocabulary() {
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    #[commands(previous = "it")]
    enum Light {
        #[command("turn {target} off")]
        Off,
    }

    trait SpokenActions: Sized {
        fn vocabulary() -> Vocabulary<Self>;
    }

    impl SpokenActions for Light {
        fn vocabulary() -> Vocabulary<Self> {
            esp_agent::vocabulary! { Light::Off => previous ["spegnila"]; }
        }
    }

    let dispatch = |_, _: &[u32]| Ok::<_, ()>(());
    let fixed = Light::handler(&LIGHTS, dispatch);
    let switchable = CommandHandler::new(&LIGHTS, Light::vocabulary(), dispatch);
    let off = Some(Command {
        action: Light::Off,
        target: TargetSelector::Previous,
    });
    assert_eq!(fixed.parse("turn it off", None), off);
    assert_eq!(fixed.parse("spegnila", None), None);
    assert_eq!(switchable.parse("turn it off", None), None);
    assert_eq!(switchable.parse("spegnila", None), off);
}

#[test]
fn default_handler_does_not_store_a_parser_pointer() {
    let fixed = Op::handler(&LIGHTS, |_, _: &[u32]| Ok::<_, ()>(()));
    assert_eq!(core::mem::size_of_val(&fixed), size_of::<&TargetCatalog>());
}

#[test]
fn manual_turn_commands_can_override_the_handler() {
    use esp_agent::TargetId;

    struct Select;
    struct CustomHandler<'a, C: ?Sized, F>(&'a C, F);

    impl<C, F, S, E> TurnHandler<Select> for CustomHandler<'_, C, F>
    where
        C: TargetLookup + ?Sized,
        F: FnMut(Select, &[TargetId]) -> Result<S, E>,
    {
        type Success = S;
        type Error = E;

        fn parse<'a>(&self, input: &'a str, _: Option<&Select>) -> Option<Command<'a, Select>> {
            Some(Command {
                action: Select,
                target: TargetSelector::Explicit(input.strip_prefix("pick ")?),
            })
        }

        fn resolve_targets(&self, selector: &str) -> TargetMatch {
            self.0.resolve_targets(selector)
        }

        fn matches_target(&self, target: TargetId, reply: &str) -> bool {
            self.0.matches_target(target, reply)
        }

        fn dispatch(&mut self, action: Select, targets: &[TargetId]) -> Result<S, E> {
            (self.1)(action, targets)
        }
    }

    impl TurnCommands for Select {
        fn parse_command(input: &str) -> Option<Command<'_, Self>> {
            Some(Command {
                action: Select,
                target: TargetSelector::Explicit(input.strip_prefix("select ")?),
            })
        }

        fn handler<F, S, E>(
            catalog: &(impl TargetLookup + ?Sized),
            dispatch: F,
        ) -> impl TurnHandler<Self, Success = S, Error = E>
        where
            F: FnMut(Self, &[TargetId]) -> Result<S, E>,
        {
            CustomHandler(catalog, dispatch)
        }
    }

    let dispatch = |_, _: &[TargetId]| Ok::<_, ()>(());
    let adapter = Select::handler(&LIGHTS, dispatch);
    let switchable = CommandHandler::new(&LIGHTS, Vocabulary::<Select>::derived(), dispatch);
    let target = Some(TargetSelector::Explicit("desk lamp"));
    for (input, direct, legacy, switched) in [
        ("select desk lamp", target, None, target),
        ("pick desk lamp", None, target, None),
    ] {
        assert_eq!(Select::parse_command(input).map(|cmd| cmd.target), direct);
        assert_eq!(adapter.parse(input, None).map(|cmd| cmd.target), legacy);
        assert_eq!(
            switchable.parse(input, None).map(|cmd| cmd.target),
            switched
        );
    }
}

#[derive(Debug, PartialEq, Eq, TurnCommands)]
enum Query {
    #[command("is {target} on?")]
    IsOn,
}

#[test]
fn pattern_punctuation_and_outer_spaces_do_not_block_a_match() {
    for (line, target) in [
        ("is it on", "it"),
        ("Is the lamp on?", "the lamp"),
        ("  is the lamp on ?! ", "the lamp"),
    ] {
        assert_eq!(
            Query::parse_command(line),
            Some(Command {
                action: Query::IsOn,
                target: TargetSelector::Explicit(target),
            }),
            "{line}"
        );
    }
}

#[derive(Debug, PartialEq, Eq, TurnCommands)]
enum Captures {
    #[command("switch {target}")]
    Switch {},
    #[command("set {{{g}}} {__parts} {c} {d} {e} {f} {a} on {target}")]
    Set {
        a: u8,
        __parts: u8,
        c: u8,
        d: u8,
        e: u8,
        f: u8,
        g: u8,
    },
    #[command("describe {target} as {label}")]
    Describe { label: String },
}

#[test]
fn commands_keep_named_fields_and_target_in_pattern_order() {
    assert_eq!(
        Captures::parse_command("switch desk"),
        Some(Command {
            action: Captures::Switch {},
            target: TargetSelector::Explicit("desk")
        })
    );
    assert_eq!(
        Captures::parse_command("set {7} 2 3 4 5 6 1 on floor lamp"),
        Some(Command {
            action: Captures::Set {
                a: 1,
                __parts: 2,
                c: 3,
                d: 4,
                e: 5,
                f: 6,
                g: 7
            },
            target: TargetSelector::Explicit("floor lamp")
        })
    );
    assert_eq!(
        Captures::parse_command("describe desk as near as possible"),
        None
    );
}

#[test]
fn typed_fields_prune_invalid_prefixes() {
    use core::str::FromStr;
    use core::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, PartialEq, Eq)]
    struct Number(u8);
    impl FromStr for Number {
        type Err = core::num::ParseIntError;
        fn from_str(text: &str) -> Result<Self, Self::Err> {
            CALLS.fetch_add(1, Ordering::Relaxed);
            text.parse().map(Self)
        }
    }
    #[derive(Debug, PartialEq, Eq, TurnCommands)]
    enum Op {
        #[command("set {target} {a} {b} {c} {d}")]
        Set {
            a: Number,
            b: Number,
            c: Number,
            d: Number,
        },
    }
    let target = "the lamp in the far corner of the living room by the door";
    let input = format!("set {target} 1 2 3 4");
    let expected = Some(Command {
        action: Op::Set {
            a: Number(1),
            b: Number(2),
            c: Number(3),
            d: Number(4),
        },
        target: TargetSelector::Explicit(target),
    });
    assert_eq!(Op::parse_command(&input), expected);
    let calls = CALLS.swap(0, Ordering::Relaxed);
    assert!(calls < 200, "{calls} derived field conversions");
    let mut words = esp_agent::RuntimeVocabulary::<Op>::new();
    words
        .insert_explicit("Set", "set {target} {a} {b} {c} {d}")
        .unwrap();
    assert_eq!(words.parse(&input), expected);
    let calls = CALLS.load(Ordering::Relaxed);
    assert!(calls < 200, "{calls} field conversions");
}

#[test]
fn adapter_runs_a_complete_agent_turn() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::new(MODEL, &knowledge).unwrap();
    let mut adapter = Op::handler(&LIGHTS, |action, ids| {
        assert_eq!(action, Op::Dim { level: Level(37) });
        assert_eq!(ids, &[1, 2]);
        Ok::<_, ()>("both dimmed")
    });
    assert_eq!(
        agent.turn("dim room to 37%", &mut adapter),
        TurnOutcome::Executed("both dimmed")
    );
}

#[test]
fn standard_handler_and_catalog_agree_on_spaced_trailing_punctuation() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::new(MODEL, &knowledge).unwrap();
    let lights = runtime_lights(discovered());
    let mut adapter = Op::handler(&lights, |action, ids| {
        assert_eq!(action, Op::Select);
        assert_eq!(ids, &[1]);
        Ok::<(), ()>(())
    });
    assert_eq!(
        agent.turn("select Desk Lamp ! ? .", &mut adapter),
        TurnOutcome::Executed(())
    );
    assert_eq!(
        lights.resolve_targets("Desk Lamp ! ? ."),
        TargetMatch::Resolved(vec![1])
    );
}

/// One discovery result, as a scan or a coordinator reports it at runtime.
struct Discovered {
    id: u32,
    name: String,
    aliases: Vec<String>,
}

fn discovered() -> Vec<Discovered> {
    vec![
        Discovered {
            id: 1,
            name: "desk lamp".into(),
            aliases: vec!["near".into(), "desk one".into()],
        },
        Discovered {
            id: 2,
            name: "floor lamp".into(),
            aliases: vec!["near".into()],
        },
    ]
}

/// Builds the runtime twin of `LIGHTS` from discovery results in any order.
fn runtime_lights(devices: impl IntoIterator<Item = Discovered>) -> DynamicTargetCatalog {
    let mut catalog = DynamicTargetCatalog::new();
    for device in devices {
        catalog
            .insert(target!(device.id, device.name, replies = device.aliases))
            .unwrap();
    }
    catalog.insert_choice(String::from("lamp"), [1, 2]).unwrap();
    catalog
        .insert_group(["room", "room lights"], [1, 2])
        .unwrap();
    catalog
}

#[test]
fn runtime_catalog_answers_like_the_static_one() {
    let empty = DynamicTargetCatalog::default();
    let lights = runtime_lights(discovered());
    for selector in [
        " THE DESK LAMP?! ",
        "floor lamp",
        "lamp",
        "the ROOM lights",
        "room",
        "desk one",
        "near",
        "garage",
        "",
        "the",
    ] {
        assert_eq!(
            empty.resolve_targets(selector),
            TargetMatch::Missing,
            "{selector}"
        );
        assert!(!empty.contains_selector(selector), "{selector}");
        assert_eq!(
            lights.resolve_targets(selector),
            LIGHTS.resolve_targets(selector),
            "{selector}"
        );
        assert_eq!(
            lights.contains_selector(selector),
            LIGHTS.contains_selector(selector),
            "{selector}"
        );
    }
    for id in [1, 2, 3] {
        assert_eq!(empty.name(id), None);
        assert_eq!(lights.name(id), LIGHTS.name(id));
        for reply in [
            "the desk lamp",
            "Desk one!",
            "near",
            "floor",
            "lamp",
            "room",
        ] {
            assert!(!empty.matches_target(id, reply));
            assert_eq!(
                lights.matches_target(id, reply),
                LIGHTS.matches_target(id, reply),
                "{id} {reply}"
            );
        }
    }

    let static_overlap = targets! {
        device Desk = 1, "desk lamp" { replies ["desk one"]; }
        device Other = 2, "desk one" {}
    };
    let mut runtime_overlap = DynamicTargetCatalog::new();
    runtime_overlap
        .insert(target!(1, "desk lamp", replies = ["desk one"]))
        .unwrap();
    runtime_overlap.insert(target!(2, "desk one")).unwrap();
    for catalog in [&static_overlap as &dyn TargetLookup, &runtime_overlap] {
        assert_eq!(
            catalog.resolve_targets("desk one"),
            TargetMatch::Resolved(vec![2])
        );
        assert!(catalog.matches_target(1, "desk one"));
    }
}

#[test]
fn runtime_catalog_rejects_conflicts_and_keeps_its_entries() {
    use CatalogError::{
        ConflictingSelector, DuplicateId, DuplicateMember, EmptyReply, EmptySelector, NoMembers,
        NoPhrases, UnknownMember,
    };
    let mut lights = runtime_lights(discovered());
    let probes = [
        "",
        "desk lamp",
        "floor lamp",
        "lamp",
        "room",
        "room lights",
        "garage",
        "hall",
        "hall lights",
        "porch",
    ];
    let snapshot = |lights: &DynamicTargetCatalog| {
        (
            probes.map(|probe| lights.resolve_targets(probe)),
            [1, 2, 9].map(|id| lights.name(id).map(String::from)),
            lights.matches_target(1, "desk one"),
        )
    };
    let before = snapshot(&lights);
    let conflict =
        |phrase: &str| -> Result<(), CatalogError> { Err(ConflictingSelector(phrase.into())) };
    type Insert = fn(&mut DynamicTargetCatalog) -> Result<(), CatalogError>;
    let cases: [(Insert, _); _] = [
        (
            |lights| lights.insert(DynamicTarget::new(1, "garage")),
            Err(DuplicateId(1)),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, "The Desk Lamp.")),
            conflict("desk lamp"),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, "Desk Lamp ! ?")),
            conflict("desk lamp"),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, "ROOM lights")),
            conflict("room lights"),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, "The Lamp")),
            conflict("lamp"),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, " ?! ")),
            Err(EmptySelector),
        ),
        (
            |lights| lights.insert(DynamicTarget::new(9, "garage").with_replies(["!"])),
            Err(EmptyReply),
        ),
        (
            |lights| lights.insert_choice("THE LAMP", [1]),
            conflict("lamp"),
        ),
        (
            |lights| lights.insert_choice("The Room Lights!", [1]),
            conflict("room lights"),
        ),
        (
            |lights| lights.insert_choice("THE FLOOR LAMP", [1]),
            conflict("floor lamp"),
        ),
        (|lights| lights.insert_choice("", [1]), Err(EmptySelector)),
        (|lights| lights.insert_choice("porch", []), Err(NoMembers)),
        (
            |lights| lights.insert_choice("porch", [2, 2]),
            Err(DuplicateMember(2)),
        ),
        (
            |lights| lights.insert_choice("porch", [1, 9]),
            Err(UnknownMember(9)),
        ),
        (
            |lights| lights.insert_group(["hall", "the HALL"], [1]),
            conflict("hall"),
        ),
        (
            |lights| lights.insert_group(["hall", "floor lamp"], [1]),
            conflict("floor lamp"),
        ),
        (
            |lights| lights.insert_group(["hall", "lamp"], [1]),
            conflict("lamp"),
        ),
        (
            |lights| lights.insert_group(["hall", "The Room Lights!"], [2]),
            conflict("room lights"),
        ),
        (
            |lights| lights.insert_group(["hall", "hall lights"], [1, 9]),
            Err(UnknownMember(9)),
        ),
        (|lights| lights.insert_group([""], [1]), Err(EmptySelector)),
        (
            |lights| lights.insert_group(Vec::<&str>::new(), [1]),
            Err(NoPhrases),
        ),
        (|lights| lights.insert_group(["hall"], []), Err(NoMembers)),
        (
            |lights| lights.insert_group(["hall"], [2, 2]),
            Err(DuplicateMember(2)),
        ),
    ];
    for (index, (insert, expected)) in cases.into_iter().enumerate() {
        assert_eq!(insert(&mut lights), expected, "case {index}");
        assert_eq!(snapshot(&lights), before, "case {index}");
    }
    assert!(lights.remove(2).is_some());
    assert_eq!(lights.resolve_targets("floor lamp"), TargetMatch::Missing);
}

#[test]
fn runtime_catalog_stops_at_a_conflicting_group_phrase() {
    let mut lights = runtime_lights(discovered());
    let phrases = ["hall", "the FLOOR LAMP"]
        .into_iter()
        .chain(core::iter::once_with(|| {
            panic!("polled past conflicting phrase")
        }));
    let members = core::iter::once_with(|| panic!("polled members after rejecting a phrase"));
    assert_eq!(
        lights.insert_group(phrases, members),
        Err(CatalogError::ConflictingSelector("floor lamp".into()))
    );
    assert_eq!(lights.resolve_targets("hall"), TargetMatch::Missing);
    assert_eq!(
        lights.resolve_targets("floor lamp"),
        TargetMatch::Resolved(vec![2])
    );
}

#[test]
fn runtime_catalog_stops_at_an_unknown_member() {
    let mut lights = runtime_lights(discovered());
    let members = || {
        [1, 9].into_iter().chain(core::iter::once_with(|| {
            panic!("polled past unknown member")
        }))
    };
    assert_eq!(
        lights.insert_choice("porch", members()),
        Err(CatalogError::UnknownMember(9))
    );
    assert_eq!(
        lights.insert_group(["hall"], members()),
        Err(CatalogError::UnknownMember(9))
    );
    assert_eq!(lights.resolve_targets("porch"), TargetMatch::Missing);
    assert_eq!(lights.resolve_targets("hall"), TargetMatch::Missing);
    assert_eq!(
        lights.resolve_targets("lamp"),
        TargetMatch::Ambiguous(vec![1, 2])
    );
    assert_eq!(
        lights.resolve_targets("room"),
        TargetMatch::Resolved(vec![1, 2])
    );
}

#[test]
fn repeated_group_phrase_diagnostic_does_not_claim_it_is_already_cataloged() {
    let mut lights = runtime_lights(discovered());
    let error = lights.insert_group(["hall", "the HALL"], [1]).unwrap_err();
    assert!(!lights.contains_selector("hall"));
    let diagnostic = error.to_string();
    assert!(diagnostic.contains("repeated"), "{diagnostic}");
}

#[test]
fn runtime_catalog_dispatches_names_and_groups_through_the_handler() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::new(MODEL, &knowledge).unwrap();
    let mut lights = DynamicTargetCatalog::new();
    for (id, name) in [
        (0, "entrée lamp"),
        (u32::MAX, "floor lamp"),
        (7, "desk lamp"),
    ] {
        lights.insert(target!(id, String::from(name))).unwrap();
    }
    lights
        .insert_group(["room lights"], [u32::MAX, 0, 7])
        .unwrap();
    let mut calls = Vec::new();
    let mut handler = Op::handler(&lights, |action, ids: &[u32]| {
        calls.push((action, ids.to_vec()));
        Ok::<_, ()>(())
    });
    assert_eq!(
        agent.turn("dim the ENTRéE LAMP to 5%", &mut handler),
        TurnOutcome::Executed(())
    );
    assert_eq!(
        agent.turn("dim room lights to 37%", &mut handler),
        TurnOutcome::Executed(())
    );
    drop(handler);
    assert_eq!(
        calls,
        [
            (Op::Dim { level: Level(5) }, vec![0]),
            (Op::Dim { level: Level(37) }, vec![u32::MAX, 0, 7])
        ]
    );
}

#[test]
fn runtime_choice_clarifies_and_a_reply_alias_answers_after_a_refresh() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let lights = runtime_lights(discovered());
    let mut handler = Op::handler(&lights, |_, _: &[u32]| -> Result<(), ()> {
        panic!("a choice must not dispatch before the user answers")
    });
    assert_eq!(
        agent.turn("dim lamp to 20%", &mut handler),
        TurnOutcome::Clarify(vec![1, 2])
    );
    drop(handler);
    // The coordinator reports the devices again, in another order and with the same IDs.
    let mut devices = discovered();
    devices[0].aliases.push("bureau près".into());
    let refreshed = runtime_lights(devices.into_iter().rev());
    let mut calls = Vec::new();
    let mut handler = Op::handler(&refreshed, |action, ids: &[u32]| {
        calls.push((action, ids.to_vec()));
        Ok::<_, ()>("dimmed")
    });
    assert_eq!(
        agent.turn("bureau près", &mut handler),
        TurnOutcome::Executed("dimmed")
    );
    drop(handler);
    assert_eq!(calls, [(Op::Dim { level: Level(20) }, vec![1])]);
}

#[test]
fn runtime_catalog_removes_and_clears_entries() {
    use TargetMatch::{Ambiguous, Missing, Resolved};
    let mut lights = runtime_lights(discovered());
    lights.insert_group(["desk corner"], [1]).unwrap();
    lights.insert_choice("reading lamp", [1]).unwrap();
    let before: Vec<_> = ["desk lamp", "lamp", "room"]
        .map(|s| lights.resolve_targets(s))
        .into();
    assert_eq!(lights.remove(9), None);
    assert_eq!(
        before,
        ["desk lamp", "lamp", "room"].map(|s| lights.resolve_targets(s))
    );
    assert_eq!(
        lights.remove(1),
        Some(DynamicTarget::new(1, "desk lamp").with_replies(["near", "desk one"]))
    );
    assert_eq!(lights.remove(1), None);
    for (selector, expected) in [
        ("desk lamp", Missing),
        ("floor lamp", Resolved(vec![2])),
        ("lamp", Ambiguous(vec![2])),
        ("room lights", Resolved(vec![2])),
        // The group and choice lost their only member and were dropped.
        ("desk corner", Missing),
        ("reading lamp", Missing),
    ] {
        assert_eq!(lights.resolve_targets(selector), expected, "{selector}");
    }
    assert_eq!(lights.name(1), None);
    assert!(!lights.matches_target(1, "desk one"));
    assert!(lights.matches_target(2, "near"));
    lights.insert_choice("reading lamp", [2]).unwrap();
    assert_eq!(lights.resolve_targets("reading lamp"), Ambiguous(vec![2]));
    // The ID, the name, and the phrase of the dropped group are free again.
    lights.insert(DynamicTarget::new(1, "desk lamp")).unwrap();
    lights.insert_group(["desk corner"], [1]).unwrap();
    assert_eq!(lights.resolve_targets("desk corner"), Resolved(vec![1]));
    lights.clear();
    for selector in ["desk lamp", "floor lamp", "lamp", "room", "desk corner"] {
        assert_eq!(lights.resolve_targets(selector), Missing, "{selector}");
    }
    assert!(
        [1, 2]
            .iter()
            .all(|&id| lights.name(id).is_none() && !lights.matches_target(id, "near"))
    );
    // A cleared catalog accepts the same entries again.
    lights.insert(DynamicTarget::new(2, "floor lamp")).unwrap();
    lights.insert_choice("lamp", [2]).unwrap();
    assert_eq!(lights.resolve_targets("lamp"), Ambiguous(vec![2]));
}

#[test]
fn refreshing_a_device_replaces_and_clears_its_reply_aliases() {
    let mut lights = runtime_lights(discovered());
    assert!(lights.matches_target(1, "desk one"));

    let desk = lights.remove(1).unwrap().with_replies(["bureau"]);
    lights.insert(desk).unwrap();
    assert!(lights.matches_target(1, "bureau"));
    assert!(!lights.matches_target(1, "desk one"));

    let desk = lights
        .remove(1)
        .unwrap()
        .with_replies(core::iter::empty::<&str>());
    lights.insert(desk).unwrap();
    assert!(!lights.matches_target(1, "bureau"));
    assert!(lights.matches_target(1, "desk lamp"));
}

fn three_lamps() -> DynamicTargetCatalog {
    let mut lights = DynamicTargetCatalog::new();
    lights.insert(target!(1, "desk lamp")).unwrap();
    lights.insert(target!(2, "floor lamp")).unwrap();
    lights
        .insert(target!(3, "wall lamp", ["wall one"]))
        .unwrap();
    lights.insert_choice("lamp", [1, 2, 3]).unwrap();
    lights.insert_group(["room"], [1, 2, 3]).unwrap();
    lights
}

#[test]
fn pending_choice_keeps_members_after_a_missing_middle_member() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(3)
        .build()
        .unwrap();
    let mut lights = three_lamps();
    let dispatch = |_, ids: &[u32]| Ok::<_, ()>(ids.to_vec());
    assert_eq!(
        agent.turn("dim lamp to 20%", &mut Op::handler(&lights, dispatch)),
        TurnOutcome::Clarify(vec![1, 2, 3])
    );
    lights.remove(2).unwrap();
    let mut handler = Op::handler(&lights, dispatch);
    assert_eq!(
        agent.turn("floor lamp", &mut handler),
        TurnOutcome::Clarify(vec![1, 3])
    );
    assert_eq!(
        agent.turn("wall one", &mut handler),
        TurnOutcome::Executed(vec![3])
    );
}

#[test]
fn remembered_group_only_offers_members_that_are_still_available() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    for (removed, previous, reply) in [
        (
            &[2][..],
            TurnOutcome::Clarify(vec![1, 3]),
            TurnOutcome::Executed(vec![3]),
        ),
        (
            &[1, 2],
            TurnOutcome::Clarify(vec![3]),
            TurnOutcome::Executed(vec![3]),
        ),
        (
            &[1, 2, 3],
            TurnOutcome::NeedTarget,
            TurnOutcome::Unrecognized,
        ),
    ] {
        let mut agent = Agent::builder(MODEL, &knowledge)
            .context(3)
            .build()
            .unwrap();
        let mut lights = three_lamps();
        let mut handler = Op::handler(&lights, |action, ids: &[u32]| {
            assert_eq!(action, Op::Select);
            assert_eq!(ids, [1, 2, 3]);
            Ok::<_, ()>(())
        });
        assert_eq!(
            agent.turn("select room", &mut handler),
            TurnOutcome::Executed(())
        );
        drop(handler);

        for &id in removed {
            lights.remove(id).unwrap();
        }
        let no_dispatch = |_, ids: &[u32]| -> Result<Vec<u32>, ()> {
            panic!("dispatched to {ids:?} before the user answered; removed {removed:?}")
        };
        assert_eq!(
            agent.turn("dim that to 20%", &mut Op::handler(&lights, no_dispatch)),
            previous,
            "removed {removed:?}"
        );
        let mut handler = Op::handler(&lights, |action, ids: &[u32]| {
            assert_eq!(action, Op::Dim { level: Level(20) });
            Ok::<_, ()>(ids.to_vec())
        });
        assert_eq!(
            agent.turn("wall one", &mut handler),
            reply,
            "removed {removed:?}"
        );
    }
}

#[test]
fn removed_remembered_device_reaches_the_callback_for_rejection() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let mut lights = runtime_lights(discovered());
    let mut handler = Op::handler(&lights, |_, _| Ok::<_, &str>(()));
    assert_eq!(
        agent.turn("select floor lamp", &mut handler),
        TurnOutcome::Executed(())
    );
    drop(handler);

    lights.remove(2).unwrap();
    let mut handler = Op::handler(&lights, |action, ids| {
        assert_eq!(action, Op::Select);
        assert_eq!(ids, [2]);
        assert_eq!(lights.name(ids[0]), None);
        Err::<(), _>("offline")
    });
    assert_eq!(
        agent.turn("select that", &mut handler),
        TurnOutcome::Failed("offline")
    );
}

#[test]
fn rediscovered_device_does_not_rejoin_a_narrowed_pending_question() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let mut lights = runtime_lights(discovered());
    let no_dispatch = |_, _: &[u32]| -> Result<(), ()> {
        panic!("a choice must not dispatch before the user answers")
    };
    assert_eq!(
        agent.turn("dim lamp to 20%", &mut Op::handler(&lights, no_dispatch)),
        TurnOutcome::Clarify(vec![1, 2])
    );
    // The floor lamp disappears from the network before the user answers.
    let floor = lights.remove(2).unwrap();
    let lookup: &dyn TargetLookup = &lights;
    assert_eq!(
        agent.turn("floor lamp", &mut Op::handler(&lookup, no_dispatch)),
        TurnOutcome::Clarify(vec![1])
    );
    // Rediscovery restores the shared alias, but the last question only offered the desk lamp.
    lights.insert(floor).unwrap();
    let mut calls = Vec::new();
    let mut handler = Op::handler(&lights, |action, ids: &[u32]| {
        calls.push((action, ids.to_vec()));
        Ok::<_, ()>(())
    });
    assert_eq!(agent.turn("near", &mut handler), TurnOutcome::Executed(()));
    drop(handler);
    assert_eq!(calls, [(Op::Dim { level: Level(20) }, vec![1])]);
}

#[test]
fn sentence_punctuation_does_not_dispatch_to_a_device_named_like_the_previous_word() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};

    #[derive(TurnCommands)]
    #[commands(previous = "it")]
    enum Action {
        #[command("select {target}")]
        Select,
    }

    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let mut catalog = DynamicTargetCatalog::new();
    catalog.insert(target!(1, "desk")).unwrap();
    catalog.insert(target!(2, "it")).unwrap();
    let mut handler = Action::handler(&catalog, |_, ids| Ok::<_, ()>(ids.to_vec()));
    for (input, expected) in [
        ("select desk", 1),
        ("select it?!", 1),
        ("select it . ! ?", 1),
        ("select the it", 2),
    ] {
        assert_eq!(
            agent.turn(input, &mut handler),
            TurnOutcome::Executed(vec![expected]),
            "{input}"
        );
    }
}

#[test]
fn configured_previous_punctuation_is_preserved_before_trailing_marks() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};

    #[derive(TurnCommands)]
    #[commands(previous = "it!")]
    enum Action {
        #[command("pick {target} now")]
        Pick,
    }

    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(1)
        .build()
        .unwrap();
    let mut catalog = DynamicTargetCatalog::new();
    catalog.insert(target!(1, "desk")).unwrap();
    catalog.insert(target!(2, "it")).unwrap();
    catalog.insert(target!(3, "ité")).unwrap();
    let mut handler = Action::handler(&catalog, |_, ids| Ok::<_, ()>(ids.to_vec()));
    for (input, expected) in [
        ("pick desk now", 1),
        ("pick it! now", 1),
        ("pick IT! ? now", 1),
        ("pick it now", 2),
        ("pick ité now", 3),
    ] {
        assert_eq!(
            agent.turn(input, &mut handler),
            TurnOutcome::Executed(vec![expected]),
            "{input}"
        );
    }
}

#[test]
fn removing_all_dynamic_choices_clears_the_pending_question() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let mut lights = runtime_lights(discovered());
    let dispatch = |_, _: &[u32]| -> Result<(), ()> { panic!("no choice was made") };
    let mut handler = Op::handler(&lights, dispatch);
    assert_eq!(
        agent.turn("dim lamp to 20%", &mut handler),
        TurnOutcome::Clarify(vec![1, 2])
    );
    drop(handler);
    lights.clear();
    let mut handler = Op::handler(&lights, dispatch);
    assert_eq!(
        agent.turn("desk one", &mut handler),
        TurnOutcome::NeedTarget
    );
    assert_eq!(agent.turn("near", &mut handler), TurnOutcome::Unrecognized);
}

#[test]
fn switching_static_catalogs_does_not_prune_pending_choices() {
    use esp_agent::{Agent, Embedder, Knowledge, Model, TurnOutcome, model};
    static MODEL: Model = model!("tests/fixtures/tiny-2d");
    static DESK_ONLY: TargetCatalog = targets! { device Desk = 1, "desk lamp" {} };
    let knowledge = Knowledge::new(Embedder::new(MODEL));
    let mut agent = Agent::builder(MODEL, &knowledge)
        .context(2)
        .build()
        .unwrap();
    let dispatch = |_, _: &[u32]| -> Result<(), ()> { panic!("no choice was made") };
    assert_eq!(
        agent.turn("dim lamp to 20%", &mut Op::handler(&LIGHTS, dispatch)),
        TurnOutcome::Clarify(vec![1, 2])
    );
    assert_eq!(
        agent.turn("floor lamp", &mut Op::handler(&DESK_ONLY, dispatch)),
        TurnOutcome::Clarify(vec![1, 2])
    );
}

#[test]
fn target_macro_owns_borrowed_temporary_arguments() {
    let id = 8;
    let entry = target!(
        id,
        format!("lamp {id}").as_str(),
        replies = [format!("alias {id}")].iter(),
    );
    let mut catalog = DynamicTargetCatalog::new();
    catalog.insert(entry).unwrap();
    assert_eq!(catalog.name(id), Some("lamp 8"));
    assert!(catalog.matches_target(id, "alias 8"));
}

#[test]
fn target_macro_accepts_positional_and_named_fields() {
    let hall = String::from("hall light");
    let hall_aliases = vec![String::from("hall one")];
    let mut catalog = DynamicTargetCatalog::new();
    for entry in [
        target!(1, "desk lamp", ["desk one"]),
        target!(2, "floor lamp"),
        target!(3, &hall, replies = &hall_aliases),
        target!(
            replies = vec!["porch one"],
            name = String::from("porch light"),
            id = 4,
        ),
        target!(5, name = "attic light", replies = ["attic one"]),
        target!(id = 6, name = "cellar light"),
        target!(
            replies = [&String::from("garden one")],
            id = 7,
            name = "garden light"
        ),
        target!(
            r#name = "kitchen light",
            r#id = 8,
            r#replies = ["kitchen one"]
        ),
    ] {
        catalog.insert(entry).unwrap();
    }
    for (id, name, alias) in [
        (1, "desk lamp", Some("desk one")),
        (2, "floor lamp", None),
        (3, "hall light", Some("hall one")),
        (4, "porch light", Some("porch one")),
        (5, "attic light", Some("attic one")),
        (6, "cellar light", None),
        (7, "garden light", Some("garden one")),
        (8, "kitchen light", Some("kitchen one")),
    ] {
        assert_eq!(catalog.name(id), Some(name));
        assert_eq!(
            catalog.resolve_targets(name),
            TargetMatch::Resolved(vec![id])
        );
        if let Some(alias) = alias {
            assert!(catalog.matches_target(id, alias), "{alias}");
            assert_eq!(catalog.resolve_targets(alias), TargetMatch::Missing);
        }
    }
}

#[test]
fn target_macro_evaluates_arguments_once_in_source_order() {
    use core::cell::Cell;

    let id = Cell::new(8);
    let mut calls = Vec::new();
    let entry = target!(
        replies = {
            calls.push("replies");
            ["second one"]
        },
        name = {
            calls.push("name");
            id.set(9);
            "second lamp"
        },
        id = {
            calls.push("id");
            id.get()
        },
    );
    let mut catalog = DynamicTargetCatalog::new();
    catalog.insert(entry).unwrap();
    assert_eq!(calls, ["replies", "name", "id"]);
    assert_eq!(
        catalog.resolve_targets("second lamp"),
        TargetMatch::Resolved(vec![9])
    );
    assert!(catalog.matches_target(9, "second one"));
}

#[test]
#[expect(non_upper_case_globals, reason = "Matches generated parameter names")]
fn target_macro_does_not_resolve_its_bindings_to_caller_constants() {
    const id: u32 = 17;
    const name: &str = "desk";
    const replies: &str = "desk one";

    let mut catalog = DynamicTargetCatalog::new();
    catalog
        .insert(target!(id - 16, format!("{name} lamp"), [replies]))
        .unwrap();
    assert_eq!(catalog.name(1), Some("desk lamp"));
    assert!(catalog.matches_target(1, "desk one"));
}

#[test]
fn target_macro_preserves_the_callers_dynamic_target_name() {
    struct DynamicTarget;
    impl DynamicTarget {
        const ID: u32 = 17;
    }

    let mut catalog = DynamicTargetCatalog::new();
    catalog
        .insert(target!(DynamicTarget::ID, "desk lamp"))
        .unwrap();
    assert_eq!(
        catalog.resolve_targets("desk lamp"),
        TargetMatch::Resolved(vec![17])
    );
}

#[test]
fn adapter_parses_with_the_vocabulary_it_was_given() {
    fn scegli(input: &str) -> Option<Command<'_, Op>> {
        input.strip_prefix("scegli ").map(|target| Command {
            action: Op::Select,
            target: TargetSelector::Explicit(target),
        })
    }
    let select = |target| {
        Some(Command {
            action: Op::Select,
            target: TargetSelector::Explicit(target),
        })
    };
    let mut adapter = CommandHandler::new(&LIGHTS, Vocabulary::<Op>::derived(), |action, ids| {
        assert_eq!(action, Op::Select);
        assert_eq!(ids, &[1]);
        Err::<(), _>("offline")
    });
    assert_eq!(adapter.parse("select desk", None), select("desk"));
    adapter.set_vocabulary(Vocabulary::new(scegli));
    assert_eq!(adapter.parse("select desk", Some(&Op::Select)), None);
    let command = adapter.parse("scegli desk", None);
    assert_eq!(command, select("desk"));
    assert_eq!(
        adapter.dispatch(command.unwrap().action, &[1]),
        Err("offline")
    );
    adapter.set_vocabulary(Vocabulary::derived());
    assert_eq!(adapter.parse("select desk", None), select("desk"));
}
