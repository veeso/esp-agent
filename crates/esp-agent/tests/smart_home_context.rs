//! Checks that an agent clarifies and remembers command targets across turns.

use core::cell::RefCell;

use esp_agent::{
    Agent, AgentError, Command, Embedder, Knowledge, Model, TargetId, TargetMatch, TargetSelector,
    TurnHandler, TurnOutcome, model,
};

static TINY: Model = model!("tests/fixtures/tiny-2d");
static OTHER: Model = model!("tests/fixtures/tiny-3d");
const DESK: TargetId = 1;
const FLOOR: TargetId = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    TurnOn,
    SetBrightness(u8),
}

#[derive(Default)]
struct House {
    calls: Vec<(Action, Vec<TargetId>)>,
    fail: bool,
    floor_removed: bool,
    suggestions: RefCell<Vec<Option<Action>>>,
}

impl TurnHandler<Action> for House {
    type Success = Action;
    type Error = &'static str;

    fn parse<'a>(&self, input: &'a str, suggested: Option<&Action>) -> Option<Command<'a, Action>> {
        self.suggestions.borrow_mut().push(suggested.cloned());
        if let Some(target) = input.strip_prefix("Turn on the ") {
            return Some(Command {
                action: Action::TurnOn,
                target: TargetSelector::Explicit(target),
            });
        }
        let (target, brightness) = input
            .strip_prefix("Dim ")?
            .strip_suffix(" percent")?
            .rsplit_once(" to ")?;
        let brightness = brightness.parse::<u8>().ok().filter(|n| *n <= 100)?;
        Some(Command {
            action: Action::SetBrightness(brightness),
            target: if target == "it" {
                TargetSelector::Previous
            } else {
                TargetSelector::Explicit(target)
            },
        })
    }

    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        match selector {
            "lamp" => TargetMatch::Ambiguous(vec![DESK, FLOOR]),
            "desk lamp" => TargetMatch::Resolved(vec![DESK]),
            "floor lamp" => TargetMatch::Resolved(vec![FLOOR]),
            "living room lights" => TargetMatch::Resolved(vec![DESK, FLOOR]),
            "empty room" => TargetMatch::Resolved(vec![]),
            "unavailable lamps" => TargetMatch::Ambiguous(vec![]),
            _ => TargetMatch::Missing,
        }
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        if target == FLOOR && self.floor_removed {
            return false;
        }
        matches!(
            (target, reply),
            (DESK, "The desk one" | "desk lamp" | "lamp")
                | (FLOOR, "The floor one" | "floor lamp" | "lamp")
        )
    }

    fn dispatch(
        &mut self,
        action: Action,
        targets: &[TargetId],
    ) -> Result<Self::Success, Self::Error> {
        self.calls.push((action.clone(), targets.to_vec()));
        if self.fail {
            Err("device unavailable")
        } else {
            Ok(action)
        }
    }
}

fn knowledge() -> Knowledge<Action> {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.insert("hello", Action::TurnOn);
    knowledge.insert("world", Action::SetBrightness(50));
    knowledge
}

fn contextual_agent(knowledge: &Knowledge<Action>, max_targets: usize) -> Agent<'_, Action> {
    Agent::builder(TINY, knowledge)
        .context(max_targets)
        .build()
        .unwrap()
}

#[test]
fn clarifies_then_reuses_target_for_another_function() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();

    assert_eq!(
        agent.turn("Turn on the lamp", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    assert!(house.calls.is_empty());
    assert_eq!(
        agent.turn("The desk one", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(
        agent.turn("Dim it to 30 percent", &mut house),
        TurnOutcome::Executed(Action::SetBrightness(30))
    );
    assert_eq!(
        house.calls,
        [
            (Action::TurnOn, vec![DESK]),
            (Action::SetBrightness(30), vec![DESK]),
        ]
    );

    assert_eq!(
        agent.turn("Turn on the living room lights", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(house.calls[2], (Action::TurnOn, vec![DESK, FLOOR]));
    assert_eq!(
        agent.turn("Dim it to 30 percent", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    assert_eq!(house.calls.len(), 3);
    assert_eq!(
        agent.turn("The floor one", &mut house),
        TurnOutcome::Executed(Action::SetBrightness(30))
    );
    assert_eq!(
        &house.calls[3..],
        [(Action::SetBrightness(30), vec![FLOOR])]
    );
}

#[test]
fn clarification_retains_the_original_brightness_argument() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();

    assert_eq!(
        agent.turn("Dim lamp to 40 percent", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    assert_eq!(
        agent.turn("The floor one", &mut house),
        TurnOutcome::Executed(Action::SetBrightness(40))
    );
    assert_eq!(house.calls, [(Action::SetBrightness(40), vec![FLOOR])]);
}

#[test]
fn custom_handler_that_does_not_report_availability_keeps_pending_choices() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();
    assert_eq!(
        agent.turn("Turn on the lamp", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    house.floor_removed = true;
    assert_eq!(
        agent.turn("The floor one", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    assert!(house.calls.is_empty());
}

#[test]
fn agent_size_is_independent_of_knowledge_payload_size() {
    assert_eq!(
        size_of::<Agent<'static, [u8; 32768]>>(),
        size_of::<Agent<'static, ()>>(),
    );
}

#[test]
fn memory_defaults_to_disabled_and_zero_disables_it() {
    let knowledge = knowledge();
    for mut agent in [
        Agent::new(TINY, &knowledge).unwrap(),
        Agent::builder(TINY, &knowledge).build().unwrap(),
        Agent::builder(TINY, &knowledge).context(0).build().unwrap(),
    ] {
        let mut house = House::default();
        assert_eq!(
            agent.turn("Turn on the lamp", &mut house),
            TurnOutcome::NeedTarget
        );
        assert_eq!(
            agent.turn("The desk one", &mut house),
            TurnOutcome::Unrecognized
        );
        assert_eq!(
            agent.turn("Turn on the desk lamp", &mut house),
            TurnOutcome::Executed(Action::TurnOn)
        );
        assert_eq!(
            agent.turn("Dim it to 30 percent", &mut house),
            TurnOutcome::NeedTarget
        );
        assert_eq!(house.calls, [(Action::TurnOn, vec![DESK])]);
    }
}

#[test]
fn refuses_to_keep_partial_choice_or_target_lists() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 1);
    let mut house = House::default();

    assert_eq!(
        agent.turn("Turn on the lamp", &mut house),
        TurnOutcome::NeedTarget
    );
    assert_eq!(
        agent.turn("The desk one", &mut house),
        TurnOutcome::Unrecognized
    );
    assert!(house.calls.is_empty());

    assert_eq!(
        agent.turn("Turn on the desk lamp", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(
        agent.turn("Turn on the living room lights", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(
        agent.turn("Dim it to 30 percent", &mut house),
        TurnOutcome::NeedTarget
    );
    assert_eq!(
        house.calls,
        [
            (Action::TurnOn, vec![DESK]),
            (Action::TurnOn, vec![DESK, FLOOR])
        ]
    );
}

#[test]
fn agents_sharing_knowledge_have_separate_conversations() {
    let knowledge = knowledge();
    let mut first = contextual_agent(&knowledge, 2);
    let mut second = contextual_agent(&knowledge, 2);
    let mut house = House::default();

    assert_eq!(
        first.turn("Turn on the lamp", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    assert_eq!(
        second.turn("The desk one", &mut house),
        TurnOutcome::Unrecognized
    );
    assert_eq!(
        first.turn("The desk one", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(
        second.turn("Dim it to 30 percent", &mut house),
        TurnOutcome::NeedTarget
    );
    assert_eq!(house.calls, [(Action::TurnOn, vec![DESK])]);
}

#[test]
fn cloned_conversations_keep_pending_actions_and_successful_targets() {
    let knowledge = knowledge();
    let mut first = contextual_agent(&knowledge, 2);
    let mut house = House::default();
    assert_eq!(
        first.turn("Turn on the lamp", &mut house),
        TurnOutcome::Clarify(vec![DESK, FLOOR])
    );
    let mut second = first.clone();
    first.turn("The desk one", &mut house);
    second.turn("The floor one", &mut house);
    let mut third = second.clone();
    first.turn("Dim it to 10 percent", &mut house);
    third.turn("Dim it to 20 percent", &mut house);
    assert_eq!(
        house.calls,
        [
            (Action::TurnOn, vec![DESK]),
            (Action::TurnOn, vec![FLOOR]),
            (Action::SetBrightness(10), vec![DESK]),
            (Action::SetBrightness(20), vec![FLOOR]),
        ]
    );
}

#[test]
fn reply_matching_both_choices_keeps_the_question_open() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();
    for input in ["Turn on the lamp", "lamp"] {
        assert_eq!(
            agent.turn(input, &mut house),
            TurnOutcome::Clarify(vec![DESK, FLOOR])
        );
    }
    assert!(house.calls.is_empty());
    assert_eq!(
        agent.turn("The desk one", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    assert_eq!(house.calls, [(Action::TurnOn, vec![DESK])]);
}

#[test]
fn unresolved_reply_keeps_the_prompt_and_new_command_replaces_it() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();

    for input in ["Turn on the lamp", "the ceiling one"] {
        assert_eq!(
            agent.turn(input, &mut house),
            TurnOutcome::Clarify(vec![DESK, FLOOR])
        );
    }
    assert!(house.calls.is_empty());
    assert_eq!(
        agent.turn("Dim floor lamp to 40 percent", &mut house),
        TurnOutcome::Executed(Action::SetBrightness(40))
    );
    assert_eq!(
        agent.turn("The desk one", &mut house),
        TurnOutcome::Unrecognized
    );
    assert_eq!(house.calls, [(Action::SetBrightness(40), vec![FLOOR])]);
}

#[test]
fn failed_dispatch_preserves_the_last_successful_target() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();
    assert_eq!(
        agent.turn("Turn on the desk lamp", &mut house),
        TurnOutcome::Executed(Action::TurnOn)
    );
    house.fail = true;
    assert_eq!(
        agent.turn("Turn on the floor lamp", &mut house),
        TurnOutcome::Failed("device unavailable")
    );
    house.fail = false;
    assert_eq!(
        agent.turn("Dim it to 30 percent", &mut house),
        TurnOutcome::Executed(Action::SetBrightness(30))
    );
    assert_eq!(
        house.calls,
        [
            (Action::TurnOn, vec![DESK]),
            (Action::TurnOn, vec![FLOOR]),
            (Action::SetBrightness(30), vec![DESK])
        ]
    );
}

#[test]
fn search_hit_is_only_a_suggestion() {
    let knowledge = knowledge();
    let mut agent = contextual_agent(&knowledge, 2);
    let mut house = House::default();

    assert_eq!(*agent.prompt("hello", 1)[0].payload, Action::TurnOn);
    assert_eq!(agent.turn("hello", &mut house), TurnOutcome::Unrecognized);
    assert_eq!(agent.turn("zzzzz", &mut house), TurnOutcome::Unrecognized);
    assert_eq!(*house.suggestions.borrow(), [Some(Action::TurnOn), None]);
    assert!(house.calls.is_empty());
}

#[test]
fn configured_builder_checks_model_compatibility() {
    let knowledge = knowledge();
    assert_eq!(
        Agent::builder(OTHER, &knowledge).context(2).build().err(),
        Some(AgentError::ModelMismatch)
    );
}

#[test]
fn explicit_selector_without_available_devices_never_dispatches() {
    let knowledge = knowledge();
    for input in [
        "Turn on the unknown lamp",
        "Turn on the empty room",
        "Turn on the unavailable lamps",
    ] {
        let mut agent = contextual_agent(&knowledge, 2);
        let mut house = House::default();
        assert_eq!(
            agent.turn(input, &mut house),
            TurnOutcome::NeedTarget,
            "{input}"
        );
        assert!(house.calls.is_empty(), "{input}");
    }
}
