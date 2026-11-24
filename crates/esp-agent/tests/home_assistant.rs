//! Exercises the firmware's serial walkthrough on the host.

extern crate alloc;

#[path = "support/allocation_counter.rs"]
mod allocation_counter;
#[path = "../../../examples/semantic-search/src/console.rs"]
mod console;
#[path = "../../../examples/semantic-search/src/home.rs"]
mod home;

use esp_agent::{Agent, Embedder, Knowledge, Model, model};

static TINY: Model = model!("tests/fixtures/tiny-2d");

#[test]
fn parsing_short_dim_commands_does_not_allocate() {
    use esp_agent::TurnHandler;

    let home = home::Home::new();
    for input in ["dim desk lamp", "dim the lamp", "dim living room lights"] {
        let allocations = allocation_counter::allocations_during(|| {
            let command = home.parse(std::hint::black_box(input), None).unwrap();
            assert_eq!(command.action, home::Action::BeginDim);
        });
        assert_eq!(allocations, 0, "{input}");
    }
}

fn say(home: &mut home::Home, agent: &mut Agent<'_, home::Action>, input: &str) -> String {
    home.respond(agent, &console::SubmittedLine::Complete(input.into()))
        .unwrap()
}

#[test]
fn brightness_question_allocates_only_the_returned_reply() {
    use esp_agent::{TurnHandler, TurnOutcome};

    let mut home = home::Home::new();
    let done = home
        .dispatch(home::Action::BeginDim, &[home::DESK])
        .unwrap();
    let outcome = TurnOutcome::Executed(done);
    let allocations = allocation_counter::allocations_during(|| {
        assert_eq!(home.reply(&outcome), "To what brightness?");
    });
    assert_eq!(allocations, 1);
}

#[test]
fn firmware_home_runs_the_serial_walkthrough() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.extend(home::EXAMPLES);
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut home = home::Home::new();
    for (input, reply, desk, floor) in [
        (
            "Dim it to 30 percent",
            "Please give the full command and name a lamp.",
            (false, 100),
            (false, 100),
        ),
        (
            "Dim the lamp to 37%",
            "Which lamp, the desk lamp or the floor lamp?",
            (false, 100),
            (false, 100),
        ),
        (
            "The desk one",
            "The desk lamp is at 37 percent.",
            (false, 37),
            (false, 100),
        ),
        (
            "Turn it on",
            "The desk lamp is on.",
            (true, 37),
            (false, 100),
        ),
        (
            "Turn on the living room lights",
            "The living room lights are on.",
            (true, 37),
            (true, 100),
        ),
        (
            "Dim it to 12%",
            "Which lamp, the desk lamp or the floor lamp?",
            (true, 37),
            (true, 100),
        ),
        (
            "Floor",
            "The floor lamp is at 12 percent.",
            (true, 37),
            (true, 12),
        ),
        (
            "Turn on it",
            "The floor lamp is on.",
            (true, 37),
            (true, 12),
        ),
        (
            "Dim it to 0 percent",
            "The floor lamp is at 0 percent.",
            (true, 37),
            (true, 0),
        ),
        (
            "Dim it to 100 percent",
            "The floor lamp is at 100 percent.",
            (true, 37),
            (true, 100),
        ),
        (
            "Dim it to 101 percent",
            "Sorry, I did not understand. Try \"Turn on the lamp\".",
            (true, 37),
            (true, 100),
        ),
        (
            "Dim it to three percent",
            "Sorry, I did not understand. Try \"Turn on the lamp\".",
            (true, 37),
            (true, 100),
        ),
    ] {
        assert_eq!(say(&mut home, &mut agent, input), reply, "{input}");
        assert_eq!(home.state(home::DESK), Some(desk), "{input}");
        assert_eq!(home.state(home::FLOOR), Some(floor), "{input}");
    }
}

#[test]
fn brightness_question_belongs_to_one_session() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.extend(home::EXAMPLES);
    let make_agent = || Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let (mut a, mut b) = (make_agent(), make_agent());
    let (mut first, mut second) = (home::Home::new(), home::Home::new());
    assert!(say(&mut first, &mut a, "Dim the lamp").starts_with("Which lamp"));
    assert!(say(&mut first, &mut a, "unrelated").starts_with("Which lamp"));
    for input in ["The desk one", "150 percent"] {
        assert_eq!(say(&mut first, &mut a, input), "To what brightness?");
        assert_eq!(first.state(home::DESK), Some((false, 100)));
    }
    say(&mut second, &mut b, "37 percent");
    assert_eq!(second.state(home::DESK), Some((false, 100)));
    assert_eq!(
        say(&mut first, &mut a, "37 percent"),
        "The desk lamp is at 37 percent."
    );
    assert_eq!(first.state(home::FLOOR), Some((false, 100)));
    say(&mut first, &mut a, "52 percent");
    assert_eq!(first.state(home::DESK), Some((false, 37)));
}

#[test]
fn brightness_question_can_only_finish_for_its_selected_device() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.extend(home::EXAMPLES);
    for line in [
        "turn on the lamp",
        "turn on the attic lamp",
        "turn on the floor lamp",
    ] {
        let mut a = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
        let mut h = home::Home::new();
        assert_eq!(say(&mut h, &mut a, "dim desk lamp"), "To what brightness?");
        say(&mut h, &mut a, line);
        say(&mut h, &mut a, "37 percent");
        assert_eq!(h.state(home::DESK), Some((false, 100)));
        assert_eq!(h.state(home::FLOOR).unwrap().1, 100);
    }
    for line in [
        "dim the floor lamp to 150%",
        "dim the attic lamp",
        "turn the lamp up",
    ] {
        let mut a = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
        let mut h = home::Home::new();
        assert_eq!(say(&mut h, &mut a, "dim desk lamp"), "To what brightness?");
        assert_ne!(say(&mut h, &mut a, line), "To what brightness?");
        say(&mut h, &mut a, "50 percent");
        assert_eq!(h.state(home::DESK), Some((false, 100)), "{line}");
        assert_eq!(h.state(home::FLOOR), Some((false, 100)), "{line}");
    }
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut home = home::Home::new();
    for (line, reply) in [
        (
            "dim desk lamp to 150%",
            r#"Sorry, I did not understand. Try "Turn on the lamp"."#,
        ),
        ("dim living room lights", "Please name one lamp to dim."),
    ] {
        assert_eq!(say(&mut home, &mut agent, line), reply);
        say(&mut home, &mut agent, "37 percent");
        assert_eq!(home.state(home::DESK), Some((false, 100)));
        assert_eq!(home.state(home::FLOOR), Some((false, 100)));
    }
    assert_eq!(
        say(&mut home, &mut agent, "dim living room lights to 29%"),
        "The living room lights are at 29 percent."
    );
    for id in [home::DESK, home::FLOOR] {
        assert_eq!(home.state(id), Some((false, 29)));
    }
}

#[test]
fn truncated_serial_input_cancels_brightness_only_for_commands() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.extend(home::EXAMPLES);
    for (prefix, expected_level) in [("turn on the floor lamp", 100), ("unrelated", 37)] {
        let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
        let mut home = home::Home::new();
        let mut editor = console::LineEditor::new();
        let mut submit = |input: &str| {
            for byte in input.bytes() {
                editor.push(byte);
            }
            assert_eq!(editor.push(b'\n'), console::Event::Submit);
            home.respond(&mut agent, &editor.take_line()).unwrap()
        };
        assert_eq!(submit("dim desk lamp"), "To what brightness?");
        let padding = " ".repeat(console::MAX_LINE_BYTES);
        assert_eq!(
            submit(&format!("{prefix}{padding}")),
            "Please enter a shorter, complete command."
        );
        submit("37 percent");
        assert_eq!(
            home.state(home::DESK),
            Some((false, expected_level)),
            "{prefix}"
        );
        assert_eq!(home.state(home::FLOOR), Some((false, 100)), "{prefix}");
    }
}

#[test]
fn serial_command_requires_the_entire_line() {
    let mut knowledge = Knowledge::new(Embedder::new(TINY));
    knowledge.extend(home::EXAMPLES);
    let mut agent = Agent::builder(TINY, &knowledge).context(2).build().unwrap();
    let mut home = home::Home::new();
    let mut editor = console::LineEditor::new();
    let command = "Turn on the desk lamp";
    let padded = format!("{command:>width$}", width = console::MAX_LINE_BYTES);
    for (input, expected, desk_on) in [
        (
            format!("{padded} and the floor lamp"),
            "Please enter a shorter, complete command.",
            false,
        ),
        (padded, "The desk lamp is on.", true),
    ] {
        for byte in input.bytes() {
            editor.push(byte);
        }
        assert_eq!(editor.push(b'\n'), console::Event::Submit);
        let line = editor.take_line();
        assert_eq!(home.respond(&mut agent, &line).as_deref(), Some(expected));
        assert_eq!(home.state(home::DESK), Some((desk_on, 100)));
        assert_eq!(home.state(home::FLOOR), Some((false, 100)));
    }
}
