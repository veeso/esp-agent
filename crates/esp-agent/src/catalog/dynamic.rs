//! The owned catalog of devices that firmware discovers at run time.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use super::{TargetLookup, normalize, same};
use crate::context::{TargetId, TargetMatch};

/// One device to insert into a [`DynamicTargetCatalog`].
///
/// Build it with [`target!`](crate::target) or with [`DynamicTarget::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicTarget {
    id: TargetId,
    name: String,
    replies: Vec<String>,
}

impl DynamicTarget {
    /// Creates a device with a stable `id` and a display `name` that also selects it in a
    /// command. The device has no reply aliases.
    #[must_use]
    pub fn new(id: TargetId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            replies: Vec::new(),
        }
    }

    /// Sets the phrases that pick the device out of a clarification only, replacing any
    /// set before. Two devices can share a reply alias.
    #[must_use]
    pub fn with_replies(mut self, replies: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.replies = replies.into_iter().map(Into::into).collect();
        self
    }
}

/// A `choose` or `group` entry: the phrases that select its members.
#[derive(Debug, Clone)]
struct Selector {
    phrases: Vec<String>,
    members: Vec<TargetId>,
}

/// Why a [`DynamicTargetCatalog`] refused an insertion.
///
/// These are the checks that `targets!` makes while the firmware compiles. A refused
/// insertion leaves the catalog unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// Another device already has this ID.
    DuplicateId(TargetId),
    /// This phrase matches an existing selector or repeats a phrase in the same insertion
    /// after normalization. The normalized phrase is written here in ASCII lowercase.
    ConflictingSelector(String),
    /// A device name, choice phrase, or group phrase is empty after normalization.
    EmptySelector,
    /// A reply alias is empty after normalization.
    EmptyReply,
    /// A group has no phrase.
    NoPhrases,
    /// A choice or group has no member.
    NoMembers,
    /// A member is not a device of the catalog.
    UnknownMember(TargetId),
    /// A member is listed twice.
    DuplicateMember(TargetId),
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "the ID {id} is already in the catalog"),
            Self::ConflictingSelector(phrase) => {
                write!(
                    f,
                    "the selector \"{phrase}\" is already in the catalog or repeated in this insertion"
                )
            }
            Self::EmptySelector => f.write_str("the selector is empty"),
            Self::EmptyReply => f.write_str("a reply alias is empty"),
            Self::NoPhrases => f.write_str("a group needs at least one phrase"),
            Self::NoMembers => f.write_str("a selector needs at least one device"),
            Self::UnknownMember(id) => write!(f, "no device in the catalog has the ID {id}"),
            Self::DuplicateMember(id) => write!(f, "the device {id} is listed twice"),
        }
    }
}

impl core::error::Error for CatalogError {}

/// The devices of a firmware that are discovered at run time, and the phrases that name
/// them.
///
/// This is the owned counterpart of [`TargetCatalog`](crate::TargetCatalog): it answers
/// the same lookups with the same precedence and normalization, but firmware fills it
/// after startup, for example from a radio scan or a coordinator. Insertions allocate,
/// and each one is checked with the rules that `targets!` applies at build time. Lookups
/// allocate only the list a [`TargetMatch`] returns.
///
/// To refresh the catalog between turns, drop the handler that borrows it,
/// [`remove`](Self::remove) the devices that vanished, [`insert`](Self::insert) the new
/// ones (or [`clear`](Self::clear) and rebuild), and build a new handler. Keep the ID of
/// every device stable, because a conversation remembers IDs and not names. A removed
/// device no longer answers a pending question, since
/// [`matches_target`](Self::matches_target) no longer finds it.
#[derive(Debug, Clone, Default)]
pub struct DynamicTargetCatalog {
    devices: Vec<DynamicTarget>,
    choices: Vec<Selector>,
    groups: Vec<Selector>,
}

impl DynamicTargetCatalog {
    /// Creates a catalog without devices.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            choices: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Adds a device.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::DuplicateId`] when another device has the ID,
    /// [`CatalogError::EmptySelector`] when the name is empty after normalization,
    /// [`CatalogError::EmptyReply`] when a reply alias is, and
    /// [`CatalogError::ConflictingSelector`] when the name equals another selector.
    pub fn insert(&mut self, target: DynamicTarget) -> Result<(), CatalogError> {
        if self.name(target.id).is_some() {
            return Err(CatalogError::DuplicateId(target.id));
        }
        self.check_selector(&target.name, &[])?;
        if target
            .replies
            .iter()
            .any(|reply| normalize(reply).is_empty())
        {
            return Err(CatalogError::EmptyReply);
        }
        self.devices.push(target);
        Ok(())
    }

    /// Makes `phrase` ambiguous between the devices `members`, so the agent asks the user
    /// to choose one.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::EmptySelector`] or [`CatalogError::ConflictingSelector`]
    /// for the phrase, and [`CatalogError::NoMembers`], [`CatalogError::UnknownMember`],
    /// or [`CatalogError::DuplicateMember`] for the members.
    pub fn insert_choice(
        &mut self,
        phrase: impl Into<String>,
        members: impl IntoIterator<Item = TargetId>,
    ) -> Result<(), CatalogError> {
        let phrase = phrase.into();
        self.check_selector(&phrase, &[])?;
        let members = self.check_members(members)?;
        self.choices.push(Selector {
            phrases: alloc::vec![phrase],
            members,
        });
        Ok(())
    }

    /// Makes each of `phrases` select all the devices `members` at once.
    ///
    /// # Errors
    ///
    /// Returns [`CatalogError::NoPhrases`] without phrases, then
    /// [`CatalogError::EmptySelector`] or [`CatalogError::ConflictingSelector`] for a
    /// phrase, which also conflicts with an earlier phrase of the same call, and
    /// [`CatalogError::NoMembers`], [`CatalogError::UnknownMember`], or
    /// [`CatalogError::DuplicateMember`] for the members.
    pub fn insert_group(
        &mut self,
        phrases: impl IntoIterator<Item = impl Into<String>>,
        members: impl IntoIterator<Item = TargetId>,
    ) -> Result<(), CatalogError> {
        let mut checked_phrases = Vec::new();
        for phrase in phrases {
            let phrase = phrase.into();
            self.check_selector(&phrase, &checked_phrases)?;
            checked_phrases.push(phrase);
        }
        if checked_phrases.is_empty() {
            return Err(CatalogError::NoPhrases);
        }
        let members = self.check_members(members)?;
        self.groups.push(Selector {
            phrases: checked_phrases,
            members,
        });
        Ok(())
    }

    /// Looks up the devices that a command's `selector` names.
    ///
    /// A device name selects that one device, a group phrase selects all its members
    /// together, and a choice phrase is ambiguous between its members. Reply aliases
    /// never select anything here. Anything else is [`TargetMatch::Missing`].
    #[must_use]
    pub fn resolve_targets(&self, selector: &str) -> TargetMatch {
        let selector = normalize(selector);
        if let Some(device) = self
            .devices
            .iter()
            .find(|device| same(&device.name, selector))
        {
            return TargetMatch::Resolved(alloc::vec![device.id]);
        }
        if let Some(group) = find(&self.groups, selector) {
            return TargetMatch::Resolved(group.members.clone());
        }
        match find(&self.choices, selector) {
            Some(choice) => TargetMatch::Ambiguous(choice.members.clone()),
            None => TargetMatch::Missing,
        }
    }

    /// Returns whether a device name, group, or choice matches the command selector.
    ///
    /// This check uses the same normalization as [`Self::resolve_targets`] and never
    /// allocates.
    #[must_use]
    pub fn contains_selector(&self, selector: &str) -> bool {
        self.selector_taken(normalize(selector), &[])
    }

    /// Tells whether `reply` picks the device `target` out of the choices of a question.
    ///
    /// The device's name and every one of its reply aliases match. Two devices can share
    /// an alias, and then both match it.
    #[must_use]
    pub fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        let reply = normalize(reply);
        self.devices.iter().any(|device| {
            device.id == target
                && (same(&device.name, reply)
                    || device.replies.iter().any(|alias| same(alias, reply)))
        })
    }

    /// Returns the display name of the device `target`, if the catalog holds it.
    #[must_use]
    pub fn name(&self, target: TargetId) -> Option<&str> {
        self.devices
            .iter()
            .find(|device| device.id == target)
            .map(|device| device.name.as_str())
    }

    /// Removes the device with the ID `target` and returns it.
    ///
    /// The ID also leaves the members of every choice and group, and a choice or group
    /// left without members is dropped. A choice left with one member stays a choice. An
    /// unknown ID returns `None` and changes nothing.
    pub fn remove(&mut self, target: TargetId) -> Option<DynamicTarget> {
        let index = self.devices.iter().position(|device| device.id == target)?;
        let device = self.devices.remove(index);
        for selectors in [&mut self.choices, &mut self.groups] {
            for selector in selectors.iter_mut() {
                selector.members.retain(|member| *member != target);
            }
            selectors.retain(|selector| !selector.members.is_empty());
        }
        Some(device)
    }

    /// Removes every device, choice, and group.
    ///
    /// Only the three outer vectors retain their capacity. The removed entries and their
    /// owned buffers are dropped.
    pub fn clear(&mut self) {
        self.devices.clear();
        self.choices.clear();
        self.groups.clear();
    }

    /// Tells whether the normalized `selector` equals a selector of the catalog or one of
    /// the `pending` phrases of the call that is being validated.
    fn selector_taken(&self, selector: &str, pending: &[String]) -> bool {
        self.devices
            .iter()
            .any(|device| same(&device.name, selector))
            || [&self.choices, &self.groups]
                .into_iter()
                .any(|selectors| find(selectors, selector).is_some())
            || pending.iter().any(|phrase| same(phrase, selector))
    }

    /// Checks that `phrase` can become a new selector next to the `pending` ones.
    fn check_selector(&self, phrase: &str, pending: &[String]) -> Result<(), CatalogError> {
        let selector = normalize(phrase);
        if selector.is_empty() {
            Err(CatalogError::EmptySelector)
        } else if self.selector_taken(selector, pending) {
            Err(CatalogError::ConflictingSelector(
                selector.to_ascii_lowercase(),
            ))
        } else {
            Ok(())
        }
    }

    /// Collects the members of a choice or group after checking that each is a distinct
    /// device of the catalog.
    fn check_members(
        &self,
        members: impl IntoIterator<Item = TargetId>,
    ) -> Result<Vec<TargetId>, CatalogError> {
        let mut checked_members = Vec::new();
        for member in members {
            if self.name(member).is_none() {
                return Err(CatalogError::UnknownMember(member));
            }
            if checked_members.contains(&member) {
                return Err(CatalogError::DuplicateMember(member));
            }
            checked_members.push(member);
        }
        if checked_members.is_empty() {
            return Err(CatalogError::NoMembers);
        }
        Ok(checked_members)
    }
}

impl TargetLookup for DynamicTargetCatalog {
    fn resolve_targets(&self, selector: &str) -> TargetMatch {
        DynamicTargetCatalog::resolve_targets(self, selector)
    }

    fn target_available(&self, target: TargetId) -> bool {
        self.name(target).is_some()
    }

    fn matches_target(&self, target: TargetId, reply: &str) -> bool {
        DynamicTargetCatalog::matches_target(self, target, reply)
    }
}

/// Finds the entry with a phrase equal to the normalized `selector`.
fn find<'a>(selectors: &'a [Selector], selector: &str) -> Option<&'a Selector> {
    selectors
        .iter()
        .find(|entry| entry.phrases.iter().any(|phrase| same(phrase, selector)))
}
