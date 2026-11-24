//! The read-only catalog of devices a firmware declares with [`targets!`](crate::targets).

use core::slice;

use crate::context::{TargetId, TargetMatch};

/// One device as `targets!` lays it out in flash.
#[derive(Debug, Clone, Copy)]
pub struct RawDevice {
    /// The stable identifier the firmware assigned.
    pub id: TargetId,
    /// The display name, which also selects the device in a command.
    pub name: &'static str,
    /// Phrases that pick the device out of a clarification only.
    pub replies: &'static [&'static str],
}

/// One `choose` or `group` declaration as `targets!` lays it out in flash.
#[derive(Debug, Clone, Copy)]
pub struct RawSelector {
    /// The phrases that select the members.
    pub phrases: &'static [&'static str],
    /// The identifiers of the member devices.
    pub members: &'static [TargetId],
}

/// The declarations of a whole catalog, as `targets!` lays them out in flash.
#[derive(Debug, Clone, Copy)]
pub struct RawCatalog {
    /// Every declared device.
    pub devices: &'static [RawDevice],
    /// Phrases that offer their members as a choice.
    pub choices: &'static [RawSelector],
    /// Phrases that select all their members at once.
    pub groups: &'static [RawSelector],
}

/// The devices of a firmware and the phrases that name them.
///
/// Build one with [`targets!`](crate::targets). The catalog is plain read-only data, so it
/// can initialize a `static`, and its lookups borrow their input and never allocate except
/// to return the owned list a [`TargetMatch`] holds. Lookups are ASCII case-insensitive,
/// ignore outer whitespace and trailing sentence punctuation, and accept one leading
/// "the".
#[derive(Debug, Clone, Copy)]
pub struct TargetCatalog {
    raw: RawCatalog,
}

/// Identifiers borrowed from the catalog before the turn takes ownership of its target list.
enum SelectorMatch {
    Resolved(&'static [TargetId]),
    Ambiguous(&'static [TargetId]),
}

impl TargetCatalog {
    /// Wraps the declarations that `targets!` compiled.
    pub(crate) const fn from_raw(raw: RawCatalog) -> Self {
        Self { raw }
    }

    /// Looks up the devices that a command's `selector` names.
    ///
    /// A device name selects that one device, a `group` phrase selects all its members
    /// together, and a `choose` phrase is ambiguous between its members. Reply aliases
    /// never select anything here, since they only make sense as an answer to a question.
    /// Anything else is [`TargetMatch::Missing`].
    #[must_use]
    pub fn resolve_targets(&self, selector: &str) -> TargetMatch {
        match self.lookup(selector) {
            Some(SelectorMatch::Resolved(ids)) => TargetMatch::Resolved(ids.to_vec()),
            Some(SelectorMatch::Ambiguous(ids)) => TargetMatch::Ambiguous(ids.to_vec()),
            None => TargetMatch::Missing,
        }
    }

    /// Returns whether a device name, group, or choice matches the command selector.
    ///
    /// This check uses the same normalization as [`Self::resolve_targets`] and never allocates.
    #[must_use]
    pub fn contains_selector(&self, selector: &str) -> bool {
        self.lookup(selector).is_some()
    }

    fn lookup(&self, selector: &str) -> Option<SelectorMatch> {
        let selector = normalize(selector);
        if let Some(device) = self
            .raw
            .devices
            .iter()
            .find(|device| same(device.name, selector))
        {
            return Some(SelectorMatch::Resolved(slice::from_ref(&device.id)));
        }
        if let Some(group) = find(self.raw.groups, selector) {
            return Some(SelectorMatch::Resolved(group.members));
        }
        find(self.raw.choices, selector).map(|choice| SelectorMatch::Ambiguous(choice.members))
    }

    /// Tells whether `reply` picks the device `target` out of the choices of a question.
    ///
    /// The device's name and every one of its reply aliases match. Two devices can share
    /// an alias, and then both match it.
    #[must_use]
    pub fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        let reply = normalize(reply);
        self.raw.devices.iter().any(|device| {
            device.id == target
                && (same(device.name, reply)
                    || device.replies.iter().any(|alias| same(alias, reply)))
        })
    }

    /// Returns the display name of the device `target`, if the catalog holds it.
    #[must_use]
    pub fn name(&self, target: TargetId) -> Option<&'static str> {
        self.raw
            .devices
            .iter()
            .find(|device| device.id == target)
            .map(|device| device.name)
    }
}

/// Finds the declaration with a phrase equal to the normalized `selector`.
fn find(selectors: &'static [RawSelector], selector: &str) -> Option<&'static RawSelector> {
    selectors
        .iter()
        .find(|entry| entry.phrases.iter().any(|phrase| same(phrase, selector)))
}

/// Tells whether `declared` is the same phrase as the already normalized `other`.
fn same(declared: &str, other: &str) -> bool {
    normalize(declared).eq_ignore_ascii_case(other)
}

/// Removes outer whitespace, trailing sentence punctuation and one leading "the".
///
/// Keep this in step with `normalize` in the macro crate, which applies it at build time.
fn normalize(text: &str) -> &str {
    let text = text.trim().trim_end_matches(['.', '!', '?']).trim_end();
    match text.get(..4) {
        Some(article) if article.eq_ignore_ascii_case("the ") => text[4..].trim_start(),
        _ => text,
    }
}
