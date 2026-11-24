//! Exercises the declarative catalog and command macros the way firmware uses them.

use esp_agent::{TargetCatalog, TargetMatch, targets};

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

use esp_agent::{Command, TargetSelector, TurnCommands, TurnHandler};

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
    assert_eq!(
        Op::parse_command(&format!("set {target} 1 2 3 4")),
        Some(Command {
            action: Op::Set {
                a: Number(1),
                b: Number(2),
                c: Number(3),
                d: Number(4)
            },
            target: TargetSelector::Explicit(target)
        })
    );
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
