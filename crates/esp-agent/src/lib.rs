//! Bare-metal building blocks for a local agent on ESP32 chips.
//!
//! The crate turns text into vectors on the chip, with static (Model2Vec-style)
//! embedding models. It is based on
//! [`static-models-bm`](https://github.com/qdrant-labs/static-models-bm) by
//! [AstraBert](https://github.com/AstraBert).
//!
//! The library is chip-agnostic: it depends on no HAL and sets up no linker
//! configuration. Add it to an existing ESP firmware project, which keeps ownership of
//! the chip setup and must provide a global allocator (for example with `esp-alloc`).
//!
//! # Embedding a model
//!
//! The library ships no model. The firmware embeds its own with [`model!`], which takes
//! a folder that holds `tokenizer.json` (a WordPiece tokenizer with the `##` continuation
//! prefix) and `model.safetensors` (one `f32` matrix with a row for every token). A
//! relative path is resolved against the crate that calls the macro, which is the firmware.
//! The macro validates the folder while the firmware compiles, and its result can
//! initialize a `static`. Call it once per model: the same firmware can embed several.
//!
//! The table is included as-is and stays in flash, where it is read in place. The
//! vocabulary is compiled into a sorted binary blob that is also read in place. The
//! firmware application partition must therefore be large enough to hold every model:
//! potion-2M takes about 7.2 MiB. A [`Scratch`] retains heap capacity for use across
//! embedding and search calls.
//!
//! # Embedding text
//!
//! An [`Embedder`] pairs a [`Model`] with an [`EmbedderConfig`] that can change at run
//! time:
//!
//! ```rust,ignore
//! use esp_agent::{Embedder, Model, model};
//!
//! static POTION: Model = model!("model");
//!
//! let mut embedder = Embedder::new(POTION);
//! embedder.config_mut().max_length = 128;
//! let vector = embedder.embed("Qdrant is a vector search engine written in Rust.");
//! ```
//!
//! # Asking an agent
//!
//! A [`Knowledge`] store pairs an [`Embedder`] with an [`Index`]. It holds texts with a
//! payload each, and any number of [`Agent`]s can borrow it. An agent takes the same
//! [`Model`] and answers a prompt with the stored texts closest to it:
//!
//! ```rust,ignore
//! use esp_agent::{Agent, Embedder, Knowledge, Model, model};
//!
//! static POTION: Model = model!("model");
//!
//! let mut knowledge = Knowledge::new(Embedder::new(POTION));
//! knowledge.insert("Paris is the capital of France.", 1);
//! knowledge.insert("Water boils at 100 degrees Celsius.", 2);
//! let agent = Agent::new(POTION, &knowledge).expect("the store is embedded with POTION");
//! let best = agent.prompt("Which city is France's capital?", 1);
//! ```
//!
//! [`Agent::new`] returns [`AgentError::ModelMismatch`] when the store was embedded with
//! another model, since vectors of two models cannot be compared.
//!
//! A prompt scores every stored entry by cosine similarity, so scores stay correct when
//! the embedder's `normalize` setting is off. Hits come back by descending score, and
//! entries with equal scores keep their insertion order. A `k` larger than the number of
//! entries returns every entry, and a `k` of zero returns none. A query with no known
//! words scores every entry `0.0`, so the hits are the first entries in insertion order.
//! An [`Index`] stores vectors you computed yourself in the same way.
//!
//! # Holding a conversation
//!
//! [`Agent::builder`] with [`context`](AgentBuilder::context) gives an agent a bounded
//! memory, and [`Agent::turn`] uses it with a [`TurnHandler`] that the firmware writes.
//! The handler parses commands from the original text, resolves targets in its own catalog,
//! and runs the actions. The agent asks which target the user means, resolves the answer,
//! and lets a later command refer to a single remembered target. A stored text is only a
//! hint for the handler and never runs a command:
//!
//! ```rust,ignore
//! let mut agent = Agent::builder(POTION, &knowledge).context(2).build()?;
//! match agent.turn("Turn on the lamp", &mut home) {
//!     TurnOutcome::Clarify(choices) => ask_which(&choices),
//!     TurnOutcome::Executed(done) => report(done),
//!     _ => {}
//! }
//! ```
//!
//! The memory is part of the agent. An agent is therefore `Clone` only when its payload is,
//! each clone has its own conversation, and an agent is no longer `Copy`.
//!
//! # Declaring devices and commands
//!
//! A handler can be written by hand, or declared. [`targets!`] builds a [`TargetCatalog`]
//! of devices, the phrases that name them, and the replies that pick one out of a
//! question. `#[derive(TurnCommands)]` turns `#[command("..")]` patterns on an action enum
//! into [`TurnCommands::parse_command`]. [`TurnCommands::handler`] joins the two with a
//! callback that runs the action on the firmware's devices:
//!
//! ```rust,ignore
//! use esp_agent::{TargetCatalog, TurnCommands, targets};
//!
//! static LAMPS: TargetCatalog = targets! {
//!     device Desk = 1, "desk lamp" { replies ["desk", "desk one"]; }
//!     device Floor = 2, "floor lamp" { replies ["floor", "floor one"]; }
//!     choose "lamp" from [Desk, Floor];
//!     group ["living room"] = [Desk, Floor];
//! };
//!
//! #[derive(TurnCommands)]
//! #[commands(previous = "it")]
//! enum Action {
//!     #[command("turn on {target}", "turn {target} on")]
//!     TurnOn,
//!     #[command("dim {target} to {level}%")]
//!     SetBrightness { level: Percent },
//! }
//!
//! let mut handler = Action::handler(&LAMPS, |action, ids| home.dispatch(action, ids));
//! let outcome = agent.turn("Dim it to 12%", &mut handler);
//! ```
//!
//! Devices that firmware learns after startup, from a scan or a coordinator, go into a
//! [`DynamicTargetCatalog`] instead. Build each entry with [`target!`], insert it, and pass
//! the catalog to [`TurnCommands::handler`] exactly like a `targets!` catalog. The handler
//! accepts any [`TargetLookup`].
//!
//! The macros and the handler are independent. A handwritten [`TurnHandler`] can call
//! [`TurnCommands::parse_command`] from its `parse`, try input of its own after it, and
//! use a catalog for the two lookups. That is the way to ask a question the agent does not
//! manage, such as the value a command is missing. The firmware example in the repository
//! asks "To what brightness?" this way. Parsing borrows the input and allocates nothing,
//! and both macros check their declarations while the firmware compiles.
//!
//! # Reusing buffers
//!
//! The one-call methods allocate fresh buffers on every call. A firmware loop can keep a
//! [`Scratch`] and a hits vector alive instead:
//!
//! ```rust,ignore
//! extern crate alloc;
//!
//! use alloc::vec::Vec;
//! use esp_agent::{Hit, Scratch};
//!
//! let mut scratch = Scratch::new();
//! let mut hits: Vec<Hit<'_, u32>> = Vec::with_capacity(3);
//! loop {
//!     agent.prompt_with(&next_question(), 3, &mut scratch, &mut hits);
//!     for hit in &hits {
//!         let text = agent.knowledge().text(hit.index).unwrap();
//!     }
//! }
//! ```
//!
//! Once the buffers are warm, [`WordPieceTokenizer::encode_with`], [`Embedder::embed_with`],
//! [`Index::search_into`], and [`Agent::prompt_with`] reuse their buffers. A call
//! allocates only if the scratch or hits vector needs to grow. Reserving room for `k`
//! hits covers any search returning up to `k` entries. Use [`Knowledge::text`] to borrow
//! the original text at a hit's insertion position.
#![no_std]

extern crate alloc;

mod agent;
mod catalog;
mod commands;
mod context;
mod embedder;
mod index;
mod knowledge;
mod model;
mod retriever;
mod scratch;
mod tokenizer;

#[doc(inline)]
pub use agent::{Agent, AgentBuilder, AgentError};
#[doc(inline)]
pub use catalog::{CatalogError, DynamicTarget, DynamicTargetCatalog, TargetCatalog, TargetLookup};
#[doc(inline)]
pub use commands::TurnCommands;
#[doc(inline)]
pub use context::{Command, TargetId, TargetMatch, TargetSelector, TurnHandler, TurnOutcome};
#[doc(inline)]
pub use embedder::{Embedder, EmbedderConfig, cosine_similarity};
#[doc(inline)]
pub use esp_agent_macros::TurnCommands;
#[doc(inline)]
pub use index::{Hit, Index};
#[doc(inline)]
pub use knowledge::Knowledge;
#[doc(inline)]
pub use model::Model;
#[doc(inline)]
pub use retriever::TextHit;
#[doc(inline)]
pub use scratch::Scratch;
#[doc(inline)]
pub use tokenizer::{WordPieceTokenizer, normalize};

#[cfg(test)]
mod test_util;

/// Embeds a WordPiece model folder in the firmware and evaluates to a `Model`.
///
/// The argument is a string literal naming a folder that holds two files:
///
/// * `tokenizer.json`: a Hugging Face WordPiece tokenizer whose continuing
///   subword prefix is `##`. Each vocabulary token must fit in 255 UTF-8 bytes.
///   The run-time tokenizer always lowercases, strips accents, and splits on
///   whitespace and punctuation like BERT, regardless of the normalizer and pre-tokenizer
///   declared in the JSON.
/// * `model.safetensors`: one matrix of finite `f32` weights with a row for every vocabulary id.
///
/// A relative path is resolved against the `CARGO_MANIFEST_DIR` of the crate
/// that calls the macro, which is the firmware crate and not `esp-agent`.
/// The folder is validated while the firmware compiles, and every problem is a
/// compile error that points at the string literal.
///
/// The expansion is a constant expression, so it can initialize a `static`.
/// The table stays in flash and is read in place.
///
/// ```rust,ignore
/// use esp_agent::{Model, model};
///
/// static POTION: Model = model!("model");
/// ```
#[macro_export]
macro_rules! model {
    ($($input:tt)*) => {{
        use $crate::_private::RawModel;
        $crate::_private::model($crate::_private::compile_model!($($input)*))
    }};
}

/// Declares the devices of a firmware and the phrases that name them, and evaluates to a
/// [`TargetCatalog`].
///
/// The body holds three kinds of declaration, in any order, each ending in a semicolon
/// except `device`, whose body is braced:
///
/// ```rust,ignore
/// use esp_agent::{TargetCatalog, targets};
///
/// static LAMPS: TargetCatalog = targets! {
///     device Desk = 1, "desk lamp" {
///         replies ["desk", "desk one"];
///     }
///     device Floor = 2, "floor lamp" {
///         replies ["floor", "floor one"];
///     }
///     choose "lamp" from [Desk, Floor];
///     group ["living room", "living room lights"] = [Desk, Floor];
/// };
/// ```
///
/// * `device Name = id, "display name" { replies [..]; }` declares a device with a stable
///   [`TargetId`], a display name that also selects it in a command, and reply aliases
///   that only pick it out of a clarification. Two devices can share a reply alias.
/// * `choose "phrase" from [..];` makes the phrase ambiguous between its members, so the
///   agent asks the user to choose one.
/// * `group ["phrase", ..] = [..];` makes each phrase select all its members at once.
///
/// The declarations are checked while the firmware compiles. Duplicate identifiers or
/// device names, selectors that collide after normalization, empty selectors, and members
/// that name no declared device are compile errors that point at the declaration. The
/// expansion is a constant expression, so it can initialize a `static`.
#[macro_export]
macro_rules! targets {
    ($($input:tt)*) => {{
        use $crate::_private::{RawCatalog, RawDevice, RawSelector};
        $crate::_private::catalog($crate::_private::compile_targets!($($input)*))
    }};
}

/// Builds a [`DynamicTarget`] from values that firmware learns at run time, to insert into a
/// [`DynamicTargetCatalog`].
///
/// The arguments are those of `format!`: positional ones first, then named ones. The
/// positional order is `id`, `name`, `replies`, and the named keys `id`, `name`, and
/// `replies` can come in any order. `replies` is optional, and a trailing comma is allowed.
///
/// ```rust,ignore
/// use esp_agent::{DynamicTargetCatalog, target};
///
/// let mut catalog = DynamicTargetCatalog::new();
/// for device in coordinator.devices() {
///     catalog.insert(target!(device.id, device.name, replies = device.aliases))?;
/// }
/// ```
///
/// * `id` is a [`TargetId`].
/// * `name` is any value that is `Into<String>`, such as a `&str`, a `String`, or a
///   `&String`.
/// * `replies` is any `IntoIterator` of values that are `Into<String>`, such as an array,
///   a `Vec<String>`, or a `&Vec<String>`.
///
/// Each expression is evaluated once, in the order written. A mistake in the argument list,
/// such as a missing ID or an unknown key, is a compile error. A conflict with the entries
/// already in the catalog, such as a duplicate ID, is a [`CatalogError`] that
/// [`DynamicTargetCatalog::insert`] returns.
#[macro_export]
macro_rules! target {
    ($($input:tt)*) => {{
        $crate::_private::compile_target!(($crate::DynamicTarget) $($input)*)
    }};
}

/// Support items for the expansion of [`model!`], [`targets!`], and [`target!`]. Not part of
/// the public API.
#[doc(hidden)]
pub mod _private {
    pub use esp_agent_macros::{compile_model, compile_target, compile_targets};

    pub use crate::catalog::{RawCatalog, RawDevice, RawSelector};
    pub use crate::commands::{CaptureMatch, selector, split, trim_sentence};

    /// Builds a [`TargetCatalog`](crate::TargetCatalog) from the declarations `targets!`
    /// compiled.
    #[must_use]
    pub const fn catalog(raw: RawCatalog) -> crate::TargetCatalog {
        crate::TargetCatalog::from_raw(raw)
    }

    /// The pieces of a compiled model folder, as laid out by `model!`.
    #[derive(Debug)]
    pub struct RawModel {
        /// The whole `model.safetensors` file, 4-byte aligned.
        pub file: &'static [u8],
        /// Byte offset of the table inside `file`.
        pub tensor_start: usize,
        /// Byte offset one past the end of the table inside `file`.
        pub tensor_end: usize,
        /// The sorted vocabulary blob.
        pub vocab: &'static [u8],
        /// Rows of the table.
        pub vocab_size: usize,
        /// Columns of the table.
        pub dim: usize,
        /// Id of the unknown token.
        pub unk_id: u32,
        /// Words longer than this many chars become the unknown token.
        pub max_chars_per_word: usize,
    }

    /// Builds a [`Model`](crate::Model) from the pieces `model!` compiled.
    ///
    /// # Panics
    ///
    /// Panics if the table's byte range is reversed or extends beyond the file,
    /// or if the table does not hold `vocab_size * dim` `f32` values.
    #[must_use]
    pub const fn model(raw: RawModel) -> crate::Model {
        crate::Model::from_raw(raw)
    }
}
