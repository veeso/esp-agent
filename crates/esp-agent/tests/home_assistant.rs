//! Exercises the firmware's serial walkthrough on the host.

extern crate alloc;

#[path = "../../../examples/semantic-search/src/console.rs"]
mod console;
#[path = "../../../examples/semantic-search/src/home.rs"]
mod home;

use esp_agent::{Agent, Embedder, Knowledge, Model, model};

static TINY: Model = model!("tests/fixtures/tiny-2d");

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
            "Turn on the lamp",
            "Which lamp, the desk lamp or the floor lamp?",
            (false, 100),
            (false, 100),
        ),
        (
            "The desk one",
            "The desk lamp is on.",
            (true, 100),
            (false, 100),
        ),
        (
            "Dim it to 30 percent",
            "The desk lamp is at 30 percent.",
            (true, 30),
            (false, 100),
        ),
        (
            "Turn on it",
            "The desk lamp is on.",
            (true, 30),
            (false, 100),
        ),
        (
            "Turn on the living room lights",
            "The living room lights are on.",
            (true, 30),
            (true, 100),
        ),
        (
            "Dim it to 30 percent",
            "Which lamp, the desk lamp or the floor lamp?",
            (true, 30),
            (true, 100),
        ),
        (
            "The floor one",
            "The floor lamp is at 30 percent.",
            (true, 30),
            (true, 30),
        ),
        (
            "Dim it to 0 percent",
            "The floor lamp is at 0 percent.",
            (true, 30),
            (true, 0),
        ),
        (
            "Dim it to 100 percent",
            "The floor lamp is at 100 percent.",
            (true, 30),
            (true, 100),
        ),
        (
            "Dim it to 101 percent",
            "Sorry, I did not understand. Try \"Turn on the lamp\".",
            (true, 30),
            (true, 100),
        ),
        (
            "Dim it to three percent",
            "Sorry, I did not understand. Try \"Turn on the lamp\".",
            (true, 30),
            (true, 100),
        ),
    ] {
        let outcome = agent.turn(input, &mut home);
        assert_eq!(home.reply(&outcome), reply, "{input}");
        assert_eq!(home.state(home::DESK), Some(desk), "{input}");
        assert_eq!(home.state(home::FLOOR), Some(floor), "{input}");
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
