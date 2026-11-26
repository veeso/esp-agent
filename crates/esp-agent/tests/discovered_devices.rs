//! Follows one discovered lamp through an English command, a language change, and a
//! follow-up in the new language. The Turns guide and the languages guide include it.

use core::convert::Infallible;
use std::collections::BTreeMap;

use esp_agent::{
    Agent, CatalogError, Command, CommandHandler, DynamicTargetCatalog, Embedder, Knowledge, Model,
    RuntimeVocabulary, TargetId, TargetSelector, TurnCommands, TurnOutcome, Vocabulary,
    VocabularyError, model, target,
};

static TINY: Model = model!("tests/fixtures/tiny-2d");

// ANCHOR: actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, TurnCommands)]
#[commands(previous = "it")]
enum Action {
    #[command("turn on {target}")]
    TurnOn,
    #[command("turn off {target}")]
    TurnOff,
}
// ANCHOR_END: actions

// ANCHOR: discovery
/// The ID and name of every device, as a coordinator would report them.
const DISCOVERED: &[(TargetId, &str)] = &[(42, "Luna")];

/// Fills a catalog with the discovered devices.
fn catalog_from(devices: &[(TargetId, &str)]) -> Result<DynamicTargetCatalog, CatalogError> {
    let mut catalog = DynamicTargetCatalog::new();
    for &(id, name) in devices {
        catalog.insert(target!(id, name))?;
    }
    Ok(catalog)
}
// ANCHOR_END: discovery

// ANCHOR: languages
/// The command phrases of every language besides English, keyed by language code.
fn languages() -> Result<BTreeMap<&'static str, RuntimeVocabulary<Action>>, VocabularyError> {
    let mut italian = RuntimeVocabulary::new();
    italian.insert_explicit("TurnOn", "accendi {target}")?;
    italian.insert_explicit("TurnOff", "spegni {target}")?;
    italian.insert_previous("TurnOn", "accendila")?;
    italian.insert_previous("TurnOff", "spegnila")?;

    let mut german = RuntimeVocabulary::new();
    german.insert_explicit("TurnOn", "schalte {target} ein")?;
    german.insert_explicit("TurnOff", "schalte {target} aus")?;
    german.insert_previous("TurnOn", "mach sie an")?;
    german.insert_previous("TurnOff", "mach sie aus")?;

    Ok(BTreeMap::from([("it", italian), ("de", german)]))
}
// ANCHOR_END: languages

// ANCHOR: handwritten
/// The derived English commands, plus “lights out” for the last lamp.
fn english(input: &str) -> Option<Command<'_, Action>> {
    if input.trim().eq_ignore_ascii_case("lights out") {
        return Some(Command {
            action: Action::TurnOff,
            target: TargetSelector::Previous,
        });
    }
    Action::parse_command(input)
}
// ANCHOR_END: handwritten

// ANCHOR: dispatch
/// The action and the devices it acted on.
type Done = (Action, Vec<TargetId>);

/// Sends the action to the devices. Real firmware talks to them here.
fn switch(action: Action, ids: &[TargetId]) -> Result<Done, Infallible> {
    Ok((action, ids.to_vec()))
}
// ANCHOR_END: dispatch

// ANCHOR: replies
/// Writes the text the firmware sends back, in the selected language.
fn reply(
    language: &str,
    catalog: &DynamicTargetCatalog,
    outcome: &TurnOutcome<Done, Infallible>,
) -> String {
    let TurnOutcome::Executed((action, ids)) = outcome else {
        return match language {
            "it" => "Scusa, non ho capito.",
            "de" => "Entschuldigung, das habe ich nicht verstanden.",
            _ => "Sorry, I did not understand.",
        }
        .to_owned();
    };
    let name = ids
        .iter()
        .filter_map(|id| catalog.name(*id))
        .collect::<Vec<_>>()
        .join(", ");
    match (language, action) {
        ("it", Action::TurnOn) => format!("{name} è accesa."),
        ("it", Action::TurnOff) => format!("{name} è spenta."),
        ("de", Action::TurnOn) => format!("{name} ist an."),
        ("de", Action::TurnOff) => format!("{name} ist aus."),
        (_, Action::TurnOn) => format!("{name} is on."),
        (_, Action::TurnOff) => format!("{name} is off."),
    }
}
// ANCHOR_END: replies

// ANCHOR: walkthrough
#[test]
fn a_discovered_lamp_keeps_its_place_across_a_language_change() {
    let catalog = catalog_from(DISCOVERED).unwrap();
    let languages = languages().unwrap();
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(1).build().unwrap();
    let mut handler = CommandHandler::new(&catalog, Vocabulary::new(english), switch);

    let outcome = agent.turn("Turn on Luna", &mut handler);
    assert_eq!(outcome, TurnOutcome::Executed((Action::TurnOn, vec![42])));
    assert_eq!(reply("en", &catalog, &outcome), "Luna is on.");

    handler.set_vocabulary(languages["it"].clone());

    let outcome = agent.turn("Spegnila", &mut handler);
    assert_eq!(outcome, TurnOutcome::Executed((Action::TurnOff, vec![42])));
    assert_eq!(reply("it", &catalog, &outcome), "Luna è spenta.");
}
// ANCHOR_END: walkthrough

#[test]
fn a_custom_phrase_acts_on_the_previous_target() {
    let catalog = catalog_from(DISCOVERED).unwrap();
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(1).build().unwrap();
    let mut handler = CommandHandler::new(&catalog, Vocabulary::new(english), switch);

    assert_eq!(
        agent.turn("Lights out", &mut handler),
        TurnOutcome::NeedTarget
    );
    assert_eq!(
        agent.turn("Turn on Luna", &mut handler),
        TurnOutcome::Executed((Action::TurnOn, vec![42]))
    );
    assert_eq!(
        agent.turn("Lights out", &mut handler),
        TurnOutcome::Executed((Action::TurnOff, vec![42]))
    );
}

#[test]
fn every_language_in_the_map_reaches_the_same_device() {
    let catalog = catalog_from(DISCOVERED).unwrap();
    let languages = languages().unwrap();
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(1).build().unwrap();
    let mut handler = CommandHandler::new(&catalog, Vocabulary::new(english), switch);

    handler.set_vocabulary(languages["de"].clone());
    let outcome = agent.turn("Schalte Luna ein", &mut handler);
    assert_eq!(outcome, TurnOutcome::Executed((Action::TurnOn, vec![42])));
    assert_eq!(reply("de", &catalog, &outcome), "Luna ist an.");
    let outcome = agent.turn("Mach sie aus", &mut handler);
    assert_eq!(outcome, TurnOutcome::Executed((Action::TurnOff, vec![42])));
    assert_eq!(reply("de", &catalog, &outcome), "Luna ist aus.");

    handler.set_vocabulary(languages["it"].clone());
    let outcome = agent.turn("Accendi Luna", &mut handler);
    assert_eq!(outcome, TurnOutcome::Executed((Action::TurnOn, vec![42])));
    assert_eq!(reply("it", &catalog, &outcome), "Luna è accesa.");
}

#[test]
fn a_replaced_language_no_longer_understands_the_old_phrases() {
    let catalog = catalog_from(DISCOVERED).unwrap();
    let languages = languages().unwrap();
    let knowledge = Knowledge::new(Embedder::new(TINY));
    let mut agent = Agent::builder(TINY, &knowledge).context(1).build().unwrap();
    let mut handler = CommandHandler::new(&catalog, Vocabulary::new(english), switch);

    handler.set_vocabulary(languages["it"].clone());
    let outcome = agent.turn("Turn on Luna", &mut handler);
    assert_eq!(outcome, TurnOutcome::Unrecognized);
    assert_eq!(reply("it", &catalog, &outcome), "Scusa, non ho capito.");

    handler.set_vocabulary(Vocabulary::new(english));
    let outcome = agent.turn("Turn on Luna", &mut handler);
    assert_eq!(reply("en", &catalog, &outcome), "Luna is on.");
}
