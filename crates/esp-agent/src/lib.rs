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
//! potion-2M takes about 7.2 MiB. The only heap use at run time is small per-request
//! scratch (normalized text, token ids, the output vector).
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
//! # Retrieving text
//!
//! A [`Retriever`] pairs an [`Embedder`] with an [`Index`]. It stores texts with a
//! payload each and returns the ones closest to a query:
//!
//! ```rust,ignore
//! use esp_agent::{Embedder, Model, Retriever, model};
//!
//! static POTION: Model = model!("model");
//!
//! let mut retriever = Retriever::new(Embedder::new(POTION));
//! retriever.insert("Paris is the capital of France.", 1);
//! retriever.insert("Water boils at 100 degrees Celsius.", 2);
//! let best = retriever.search("Which city is France's capital?", 1);
//! ```
//!
//! A search scores every stored entry by cosine similarity, so scores stay correct when
//! the embedder's `normalize` setting is off. Hits come back by descending score, and
//! entries with equal scores keep their insertion order. A `k` larger than the number of
//! entries returns every entry, and a `k` of zero returns none. A query with no known
//! words scores every entry `0.0`, so the hits are the first entries in insertion order.
//! An [`Index`] stores vectors you computed yourself in the same way.
#![no_std]

extern crate alloc;

mod embedder;
mod index;
mod model;
mod retriever;
mod tokenizer;

#[doc(inline)]
pub use embedder::{Embedder, EmbedderConfig, cosine_similarity};
#[doc(inline)]
pub use index::{Hit, Index};
#[doc(inline)]
pub use model::Model;
#[doc(inline)]
pub use retriever::{Retriever, TextHit};
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

/// Support items for the expansion of [`model!`]. Not part of the public API.
#[doc(hidden)]
pub mod _private {
    pub use esp_agent_macros::compile_model;

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
