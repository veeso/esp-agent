//! The simulated smart home: a catalog of two lamps, the command vocabulary, and the replies.
//!
//! Nothing here touches hardware, so the logic builds on any target.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use esp_agent::{Agent, Command, TargetId, TargetMatch, TargetSelector, TurnHandler, TurnOutcome};

use crate::console::SubmittedLine;

/// Identifier of the desk lamp in the catalog.
pub const DESK: TargetId = 1;
/// Identifier of the floor lamp in the catalog.
pub const FLOOR: TargetId = 2;

// ANCHOR: examples
/// Texts registered in the knowledge store, one per supported action.
pub const EXAMPLES: [(&str, Action); 2] = [
    ("Turn on the lamp", Action::TurnOn),
    ("Dim the lamp to 30 percent", Action::SetBrightness(30)),
];

/// What the user can ask a lamp to do, with the arguments read from the original input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Switch the lamp on.
    TurnOn,
    /// Set the brightness, in percent from 0 to 100.
    SetBrightness(u8),
}
// ANCHOR_END: examples

/// Why a command could not run on the simulated devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeError {
    /// The command named no target.
    NoTarget,
    /// The identifier is not in the catalog.
    UnknownTarget(TargetId),
    /// The brightness is above 100 percent.
    InvalidBrightness(u8),
}

/// A command that ran, with the targets it ran on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    action: Action,
    targets: Vec<TargetId>,
}

/// The state of one simulated lamp.
#[derive(Debug, Clone, Copy)]
struct Lamp {
    id: TargetId,
    name: &'static str,
    /// Phrases that pick this lamp out of a clarification, compared ignoring case.
    replies: &'static [&'static str],
    on: bool,
    brightness: u8,
}

/// The simulated home that the agent controls.
#[derive(Debug)]
pub struct Home {
    lamps: [Lamp; 2],
}

impl Home {
    /// Creates a home with both lamps off at full brightness.
    pub const fn new() -> Self {
        Self {
            lamps: [
                Lamp {
                    id: DESK,
                    name: "desk lamp",
                    replies: &[
                        "the desk one",
                        "desk one",
                        "the desk lamp",
                        "desk lamp",
                        "desk",
                    ],
                    on: false,
                    brightness: 100,
                },
                Lamp {
                    id: FLOOR,
                    name: "floor lamp",
                    replies: &[
                        "the floor one",
                        "floor one",
                        "the floor lamp",
                        "floor lamp",
                        "floor",
                    ],
                    on: false,
                    brightness: 100,
                },
            ],
        }
    }

    /// Handles a submitted serial line and returns the reply to print, if any.
    pub fn respond(
        &mut self,
        agent: &mut Agent<'_, Action>,
        line: &SubmittedLine,
    ) -> Option<String> {
        if matches!(line, SubmittedLine::Truncated(_)) {
            return Some(String::from("Please enter a shorter, complete command."));
        }
        let input = line.text().trim();
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

    fn lamp_mut(&mut self, id: TargetId) -> Option<&mut Lamp> {
        self.lamps.iter_mut().find(|lamp| lamp.id == id)
    }

    fn all_ids(&self) -> Vec<TargetId> {
        self.lamps.iter().map(|lamp| lamp.id).collect()
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
            names.push_str(self.lamp(*id).map_or("device", |lamp| lamp.name));
        }
        names
    }

    /// Writes the reply to `outcome` in plain English.
    pub fn reply(&self, outcome: &TurnOutcome<Done, HomeError>) -> String {
        let mut reply = String::new();
        match outcome {
            TurnOutcome::Executed(done) => {
                let subject = capitalize(&self.name_of(&done.targets));
                let verb = if done.targets.len() == 1 { "is" } else { "are" };
                let _ = match done.action {
                    Action::TurnOn => write!(reply, "{subject} {verb} on."),
                    Action::SetBrightness(percent) => {
                        write!(reply, "{subject} {verb} at {percent} percent.")
                    }
                };
            }
            TurnOutcome::Failed(error) => {
                let _ = match error {
                    HomeError::NoTarget => write!(reply, "That command has no lamp."),
                    HomeError::UnknownTarget(id) => write!(reply, "I do not know device {id}."),
                    HomeError::InvalidBrightness(percent) => {
                        write!(reply, "{percent} percent is not a valid brightness.")
                    }
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
                        name = self.lamp(*id).map_or("device", |lamp| lamp.name)
                    );
                }
                reply.push('?');
            }
            TurnOutcome::NeedTarget => {
                reply.push_str("Please give the full command and name a lamp.")
            }
            TurnOutcome::Unrecognized => {
                reply.push_str("Sorry, I did not understand. Try \"Turn on the lamp\".");
            }
        }
        reply
    }
}

// ANCHOR: handler
impl TurnHandler<Action> for Home {
    type Success = Done;
    type Error = HomeError;

    fn parse<'a>(
        &self,
        input: &'a str,
        _suggested: Option<&Action>,
    ) -> Option<Command<'a, Action>> {
        let input = trim_sentence(input);
        if let Some(target) = strip_prefix_ignore_case(input, "turn on ") {
            return Some(Command {
                action: Action::TurnOn,
                target: parse_selector(target),
            });
        }
        let rest = strip_prefix_ignore_case(input, "dim ")?;
        let rest = strip_suffix_ignore_case(rest, " percent")
            .or_else(|| strip_suffix_ignore_case(rest, "%"))?;
        let (target, percent) = rsplit_once_ignore_case(rest, " to ")?;
        let percent = percent.trim().parse::<u8>().ok().filter(|n| *n <= 100)?;
        Some(Command {
            action: Action::SetBrightness(percent),
            target: parse_selector(target),
        })
    }

    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        let selector = trim_sentence(selector);
        let selector = strip_prefix_ignore_case(selector, "the ").unwrap_or(selector);
        let is = |name: &str| selector.eq_ignore_ascii_case(name);
        if is("lamp") {
            TargetMatch::Ambiguous(self.all_ids())
        } else if is("desk lamp") {
            TargetMatch::Resolved(Vec::from([DESK]))
        } else if is("floor lamp") {
            TargetMatch::Resolved(Vec::from([FLOOR]))
        } else if is("living room lights") || is("living room") {
            TargetMatch::Resolved(self.all_ids())
        } else {
            TargetMatch::Missing
        }
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        let reply = trim_sentence(reply);
        self.lamp(target).is_some_and(|lamp| {
            lamp.replies
                .iter()
                .any(|phrase| reply.eq_ignore_ascii_case(phrase))
        })
    }

    fn dispatch(&mut self, action: Action, targets: &[TargetId]) -> Result<Done, HomeError> {
        if targets.is_empty() {
            return Err(HomeError::NoTarget);
        }
        if let Some(unknown) = targets.iter().find(|id| self.lamp(**id).is_none()) {
            return Err(HomeError::UnknownTarget(*unknown));
        }
        if let Action::SetBrightness(percent) = action
            && percent > 100
        {
            return Err(HomeError::InvalidBrightness(percent));
        }
        for id in targets {
            if let Some(lamp) = self.lamp_mut(*id) {
                match action {
                    Action::TurnOn => lamp.on = true,
                    Action::SetBrightness(percent) => lamp.brightness = percent,
                }
            }
        }
        Ok(Done {
            action,
            targets: targets.to_vec(),
        })
    }
}
// ANCHOR_END: handler

fn parse_selector(text: &str) -> TargetSelector<'_> {
    let target = text.trim();
    if target.eq_ignore_ascii_case("it") {
        TargetSelector::Previous
    } else {
        TargetSelector::Explicit(target)
    }
}

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

fn rsplit_once_ignore_case<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let start = (0..=text.len().checked_sub(separator.len())?)
        .rev()
        .find(|start| {
            text.get(*start..*start + separator.len())
                .is_some_and(|part| part.eq_ignore_ascii_case(separator))
        })?;
    Some((&text[..start], &text[start + separator.len()..]))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
