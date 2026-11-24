//! The simulated smart home: a catalog of two lamps, the command vocabulary, and the replies.
//!
//! Nothing here touches hardware, so the logic builds on any target.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;
use core::str::FromStr;

use esp_agent::{
    Agent, Command, TargetCatalog, TargetId, TargetMatch, TargetSelector, TurnCommands,
    TurnHandler, TurnOutcome, targets,
};

use crate::console::SubmittedLine;

/// Identifier of the desk lamp in the catalog.
pub const DESK: TargetId = 1;
/// Identifier of the floor lamp in the catalog.
pub const FLOOR: TargetId = 2;

// ANCHOR: catalog
/// The lamps, the phrases that name them, and the replies that pick one out of a question.
static LAMPS: TargetCatalog = targets! {
    device Desk = 1, "desk lamp" {
        replies ["desk", "desk one"];
    }
    device Floor = 2, "floor lamp" {
        replies ["floor", "floor one"];
    }
    choose "lamp" from [Desk, Floor];
    group ["living room", "living room lights"] = [Desk, Floor];
};
// ANCHOR_END: catalog

// ANCHOR: examples
/// Knowledge store examples for turning a lamp on and setting its brightness.
pub const EXAMPLES: [(&str, Action); 2] = [
    ("Turn on the lamp", Action::TurnOn),
    (
        "Dim the lamp to 30 percent",
        Action::SetBrightness {
            level: Percent::new(30),
        },
    ),
];

/// What the user can ask a lamp to do, with the arguments read from the original input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, TurnCommands)]
#[commands(previous = "it")]
pub enum Action {
    /// Switch the lamp on.
    #[command("turn on {target}", "turn {target} on")]
    TurnOn,
    /// Set the brightness.
    #[command("dim {target} to {level} percent", "dim {target} to {level}%")]
    SetBrightness {
        /// The new brightness.
        level: Percent,
    },
    /// Ask which brightness the user wants. The firmware builds it from a shorter command.
    BeginDim,
}

/// A brightness in percent, from 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Percent(u8);

impl Percent {
    /// Creates a brightness at compile time.
    ///
    /// # Panics
    ///
    /// Panics if `value` is above 100.
    pub const fn new(value: u8) -> Self {
        assert!(value <= 100, "a percentage is at most 100");
        Self(value)
    }
}

impl FromStr for Percent {
    type Err = ();

    /// Accepts decimal digits only, from 0 through 100.
    fn from_str(text: &str) -> Result<Self, ()> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(());
        }
        text.parse::<u8>()
            .ok()
            .filter(|value| *value <= 100)
            .map(Self)
            .ok_or(())
    }
}
// ANCHOR_END: examples

/// Why a command could not run on the simulated devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeError {
    /// The command named no target.
    NoTarget,
    /// The identifier is not in the catalog.
    UnknownTarget(TargetId),
    /// The brightness question needs exactly one lamp.
    NameOneLamp,
}

/// A command that ran, with the targets it ran on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    action: Action,
    targets: Vec<TargetId>,
}

/// The state of one simulated lamp. Its name and replies live in the catalog.
#[derive(Debug, Clone, Copy)]
struct Lamp {
    id: TargetId,
    on: bool,
    brightness: u8,
}

/// The simulated home that one conversation controls.
#[derive(Debug)]
pub struct Home {
    lamps: [Lamp; 2],
    /// The lamp whose brightness the user was asked for, while the question is open.
    awaiting_brightness: Option<TargetId>,
}

impl Home {
    /// Creates a home with both lamps off at full brightness.
    pub const fn new() -> Self {
        Self {
            lamps: [
                Lamp {
                    id: DESK,
                    on: false,
                    brightness: 100,
                },
                Lamp {
                    id: FLOOR,
                    on: false,
                    brightness: 100,
                },
            ],
            awaiting_brightness: None,
        }
    }

    /// Handles a submitted serial line and returns the reply to print, if any.
    pub fn respond(
        &mut self,
        agent: &mut Agent<'_, Action>,
        line: &SubmittedLine,
    ) -> Option<String> {
        let input = line.text().trim_start();
        // A new command abandons the question even if the editor truncated the line.
        if self.awaiting_brightness.is_some() && starts_like_command(input) {
            self.awaiting_brightness = None;
        }
        if matches!(line, SubmittedLine::Truncated(_)) {
            return Some(String::from("Please enter a shorter, complete command."));
        }
        let input = input.trim_end();
        if input.is_empty() {
            return None;
        }
        let outcome = agent.turn(input, self);
        Some(self.reply(&outcome))
    }

    #[cfg(test)]
    pub fn state(&self, id: TargetId) -> Option<(bool, u8)> {
        self.lamp(id).map(|lamp| (lamp.on, lamp.brightness))
    }

    fn lamp(&self, id: TargetId) -> Option<&Lamp> {
        self.lamps.iter().find(|lamp| lamp.id == id)
    }

    /// Words the user can say for a target, such as "the desk lamp".
    fn name_of(&self, targets: &[TargetId]) -> String {
        if targets.len() == self.lamps.len()
            && self.lamps.iter().all(|lamp| targets.contains(&lamp.id))
        {
            return String::from("the living room lights");
        }
        let mut names = String::new();
        for (position, id) in targets.iter().enumerate() {
            if position > 0 {
                names.push_str(" and ");
            }
            names.push_str("the ");
            names.push_str(LAMPS.name(*id).unwrap_or("device"));
        }
        names
    }

    /// Writes the reply to `outcome` in plain English.
    pub fn reply(&self, outcome: &TurnOutcome<Done, HomeError>) -> String {
        let mut reply = String::new();
        match outcome {
            TurnOutcome::Executed(done) => {
                let subject = || capitalize(&self.name_of(&done.targets));
                let verb = if done.targets.len() == 1 { "is" } else { "are" };
                let _ = match done.action {
                    Action::TurnOn => write!(reply, "{subject} {verb} on.", subject = subject()),
                    Action::SetBrightness { level } => {
                        write!(
                            reply,
                            "{subject} {verb} at {level} percent.",
                            subject = subject(),
                            level = level.0
                        )
                    }
                    Action::BeginDim => return String::from(BRIGHTNESS_QUESTION),
                };
            }
            TurnOutcome::Failed(error) => {
                let _ = match error {
                    HomeError::NoTarget => write!(reply, "That command has no lamp."),
                    HomeError::UnknownTarget(id) => write!(reply, "I do not know device {id}."),
                    HomeError::NameOneLamp => write!(reply, "Please name one lamp to dim."),
                };
            }
            TurnOutcome::Clarify(choices) => {
                let _ = write!(reply, "Which lamp, ");
                for (position, id) in choices.iter().enumerate() {
                    if position > 0 {
                        reply.push_str(if position + 1 == choices.len() {
                            " or "
                        } else {
                            ", "
                        });
                    }
                    let _ = write!(
                        reply,
                        "the {name}",
                        name = LAMPS.name(*id).unwrap_or("device")
                    );
                }
                reply.push('?');
            }
            TurnOutcome::NeedTarget => {
                reply.push_str("Please give the full command and name a lamp.")
            }
            TurnOutcome::Unrecognized if self.awaiting_brightness.is_some() => {
                reply.push_str(BRIGHTNESS_QUESTION);
            }
            TurnOutcome::Unrecognized => {
                reply.push_str("Sorry, I did not understand. Try \"Turn on the lamp\".");
            }
        }
        reply
    }
}

/// What the firmware asks after "dim <lamp>".
const BRIGHTNESS_QUESTION: &str = "To what brightness?";

// ANCHOR: brightness_followup
/// Reads a complete command, or the short "dim <lamp>" that starts the brightness question.
fn parse_command(input: &str) -> Option<Command<'_, Action>> {
    Action::parse_command(input).or_else(|| begin_dim(input))
}

/// Recognizes "dim <lamp>" when the lamp is a known selector or the word "it".
fn begin_dim(input: &str) -> Option<Command<'_, Action>> {
    let rest = strip_prefix_ignore_case(trim_sentence(input), "dim ")?.trim();
    let target = if rest.eq_ignore_ascii_case("it") {
        TargetSelector::Previous
    } else if LAMPS.contains_selector(rest) {
        TargetSelector::Explicit(rest)
    } else {
        return None;
    };
    Some(Command {
        action: Action::BeginDim,
        target,
    })
}

/// Tells whether the line opens with a command verb, whether or not it is a valid command.
fn starts_like_command(input: &str) -> bool {
    ["dim ", "turn "]
        .iter()
        .any(|verb| strip_prefix_ignore_case(input, verb).is_some())
}

/// Reads a bare brightness such as "37 percent" or "37%".
fn parse_level(input: &str) -> Option<Percent> {
    let input = trim_sentence(input);
    strip_suffix_ignore_case(input, " percent")
        .or_else(|| strip_suffix_ignore_case(input, "%"))?
        .trim()
        .parse()
        .ok()
}
// ANCHOR_END: brightness_followup

// ANCHOR: handler
impl TurnHandler<Action> for Home {
    type Success = Done;
    type Error = HomeError;

    fn parse<'a>(
        &self,
        input: &'a str,
        _suggested: Option<&Action>,
    ) -> Option<Command<'a, Action>> {
        parse_command(input).or_else(|| {
            let lamp = self.awaiting_brightness?;
            Some(Command {
                action: Action::SetBrightness {
                    level: parse_level(input)?,
                },
                target: TargetSelector::Explicit(LAMPS.name(lamp)?),
            })
        })
    }

    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        LAMPS.resolve_targets(selector)
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        LAMPS.matches_target(target, reply)
    }

    fn dispatch(&mut self, action: Action, targets: &[TargetId]) -> Result<Done, HomeError> {
        if targets.is_empty() {
            return Err(HomeError::NoTarget);
        }
        if let Some(unknown) = targets.iter().find(|id| self.lamp(**id).is_none()) {
            return Err(HomeError::UnknownTarget(*unknown));
        }
        if action == Action::BeginDim {
            let &[lamp] = targets else {
                return Err(HomeError::NameOneLamp);
            };
            self.awaiting_brightness = Some(lamp);
        } else {
            for lamp in self
                .lamps
                .iter_mut()
                .filter(|lamp| targets.contains(&lamp.id))
            {
                match action {
                    Action::SetBrightness { level } => lamp.brightness = level.0,
                    Action::TurnOn | Action::BeginDim => lamp.on = true,
                }
            }
            self.awaiting_brightness = None;
        }
        Ok(Done {
            action,
            targets: targets.to_vec(),
        })
    }
}
// ANCHOR_END: handler

/// Removes surrounding whitespace and trailing sentence punctuation.
fn trim_sentence(text: &str) -> &str {
    text.trim().trim_end_matches(['.', '!', '?']).trim_end()
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let start = text.len().checked_sub(suffix.len())?;
    text.get(start..)?
        .eq_ignore_ascii_case(suffix)
        .then(|| &text[..start])
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
