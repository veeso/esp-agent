//! A `no_std` BERT WordPiece tokenizer.
//!
//! It reproduces the pipeline declared in `model/tokenizer.json`:
//!
//! 1. `BertNormalizer` (`clean_text`, `handle_chinese_chars`, accent stripping, lowercasing)
//! 2. `BertPreTokenizer` (split on whitespace, isolate every punctuation character)
//! 3. `WordPiece` (greedy longest-match-first, `##` continuation prefix, whole word
//!    becomes `[UNK]` if any piece is missing or if it is longer than the word limit)
//!
//! No special tokens are added, which is what static (Model2Vec) embedding models expect.
//!
//! The vocabulary is **not** parsed or copied: `build.rs` compiles it into a sorted binary
//! blob (see [`Vocab`]) that is binary-searched where it lives, in flash.

use alloc::string::String;
use alloc::vec::Vec;

use unicode_normalization::UnicodeNormalization;
use unicode_properties::{GeneralCategory, GeneralCategoryGroup, UnicodeGeneralCategory};

const CONTINUING_PREFIX: &str = "##";

fn read_u32(blob: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([blob[at], blob[at + 1], blob[at + 2], blob[at + 3]])
}

/// A read-only vocabulary stored as a sorted, offset-indexed blob.
///
/// Layout (little-endian): `u32 count`, `count x u32` entry offsets ordered by token
/// bytes, then entries made of `u32 id`, `u8 len`, token bytes.
#[derive(Clone, Copy)]
pub struct Vocab {
    blob: &'static [u8],
}

impl Vocab {
    pub const fn new(blob: &'static [u8]) -> Self {
        Self { blob }
    }

    pub fn len(&self) -> usize {
        read_u32(self.blob, 0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn entry(&self, index: usize) -> (&'static [u8], u32) {
        let off = read_u32(self.blob, 4 + 4 * index) as usize;
        let id = read_u32(self.blob, off);
        let len = self.blob[off + 4] as usize;
        (&self.blob[off + 5..off + 5 + len], id)
    }

    /// Returns the id of `token`, if it is in the vocabulary.
    pub fn get(&self, token: &str) -> Option<u32> {
        let key = token.as_bytes();
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let (candidate, id) = self.entry(mid);
            match candidate.cmp(key) {
                core::cmp::Ordering::Equal => return Some(id),
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
            }
        }
        None
    }
}

/// BERT-style WordPiece tokenizer over an in-flash [`Vocab`].
pub struct WordPieceTokenizer {
    vocab: Vocab,
    unk_id: u32,
    max_chars_per_word: usize,
    max_length: usize,
}

impl WordPieceTokenizer {
    /// * `max_chars_per_word`: words longer than this (in chars) become `[UNK]`.
    /// * `max_length`: truncation, counted in tokens (including `[UNK]`s), like the
    ///   reference implementation which truncates before `[UNK]`s are removed.
    pub const fn new(
        vocab: Vocab,
        unk_id: u32,
        max_chars_per_word: usize,
        max_length: usize,
    ) -> Self {
        Self {
            vocab,
            unk_id,
            max_chars_per_word,
            max_length,
        }
    }

    pub fn unk_id(&self) -> u32 {
        self.unk_id
    }

    /// Tokenizes `text` and returns its token ids, **without** the `[UNK]` ids
    /// (a static embedding model ignores them).
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let normalized = normalize(text);
        let mut ids = Vec::new();
        self.encode_normalized(&normalized, &mut ids, false);
        ids
    }

    /// Like [`encode`](Self::encode), but keeps `[UNK]` ids in the output.
    pub fn encode_with_unk(&self, text: &str) -> Vec<u32> {
        let normalized = normalize(text);
        let mut ids = Vec::new();
        self.encode_normalized(&normalized, &mut ids, true);
        ids
    }

    /// Splits already-normalized text into words and WordPieces each one.
    ///
    /// Example: "hello, world" -> words "hello" "," "world" -> one `word_piece` call each.
    fn encode_normalized(&self, text: &str, ids: &mut Vec<u32>, keep_unk: bool) {
        // Reused buffer for building vocab lookup keys, so we don't allocate per lookup.
        let mut scratch = String::new();
        // Tokens produced so far, [UNK] included. Used only to enforce `max_length`.
        let mut emitted = 0usize;
        // Byte index where the word currently being scanned began (None between words).
        let mut word_start: Option<usize> = None;

        // BertPreTokenizer: whitespace separates words and is dropped; every punctuation
        // character is a word of its own.
        for (i, c) in text.char_indices() {
            let is_space = c.is_whitespace();
            let is_punct = !is_space && is_bert_punctuation(c);
            if is_space || is_punct {
                // A separator ends the word we were scanning, if any.
                if let Some(start) = word_start.take() {
                    self.word_piece(&text[start..i], &mut scratch, ids, &mut emitted, keep_unk);
                }
                // Punctuation is not just a separator: it is also a one-char word itself.
                if is_punct {
                    self.word_piece(
                        &text[i..i + c.len_utf8()],
                        &mut scratch,
                        ids,
                        &mut emitted,
                        keep_unk,
                    );
                }
            } else if word_start.is_none() {
                // First ordinary char after a separator: a new word starts here.
                word_start = Some(i);
            }
            // Truncation reached: nothing more can be added.
            if emitted >= self.max_length {
                return;
            }
        }
        // The text may end in the middle of a word (no trailing separator): flush it.
        if let Some(start) = word_start {
            self.word_piece(&text[start..], &mut scratch, ids, &mut emitted, keep_unk);
        }
    }

    /// Greedy longest-match-first WordPiece over one pre-tokenized word.
    ///
    /// Repeatedly take the longest prefix of the remaining text that is in the vocab.
    /// Pieces after the first are looked up with a `##` prefix. Example, with
    /// "unaffable" and vocab {"un", "##aff", "##able"}:
    /// try "unaffable".."una" (miss) -> "un" (hit); then "##affable".."##af" (miss)
    /// -> "##aff" (hit); then "##able" (hit) => [un, ##aff, ##able].
    /// If at any point not even a single char matches, the *whole word* becomes [UNK].
    fn word_piece(
        &self,
        word: &str,
        scratch: &mut String,
        ids: &mut Vec<u32>,
        emitted: &mut usize,
        keep_unk: bool,
    ) {
        // Where this word's ids begin in `ids`, so we can roll them back on failure.
        let first_id = ids.len();
        // Number of tokens this word yields (pieces, or 1 for [UNK]).
        let mut produced = 0usize;
        // Too-long words are [UNK] without even trying.
        let mut ok = word.chars().count() <= self.max_chars_per_word;

        // `start..end` is the candidate piece (byte offsets into `word`).
        let mut start = 0;
        while ok && start < word.len() {
            // Begin with the longest candidate (everything left) and shrink it.
            let mut end = word.len();
            let mut found = None;
            while end > start {
                scratch.clear();
                if start > 0 {
                    scratch.push_str(CONTINUING_PREFIX);
                }
                scratch.push_str(&word[start..end]);
                if let Some(id) = self.vocab.get(scratch) {
                    found = Some(id);
                    break;
                }
                // Shrink by one char (step back to a UTF-8 boundary, not a raw byte).
                end -= 1;
                while !word.is_char_boundary(end) {
                    end -= 1;
                }
            }
            match found {
                // Piece matched: emit it and continue after it.
                Some(id) => {
                    ids.push(id);
                    produced += 1;
                    start = end;
                }
                // Nothing matched, not even one char: give up on the whole word.
                None => ok = false,
            }
        }

        if !ok {
            // The whole word is unknown, not just the piece that failed.
            ids.truncate(first_id);
            produced = 1;
            if keep_unk {
                ids.push(self.unk_id);
            }
        }

        // Enforce `max_length` at the token level, even in the middle of a word.
        // `emitted` counts [UNK]s too (even when `ids` omits them), matching the reference,
        // which truncates before [UNK]s are removed.
        let remaining = self.max_length - *emitted;
        if produced > remaining {
            let kept = if ok || keep_unk { remaining } else { 0 };
            ids.truncate(first_id + kept.min(ids.len() - first_id));
            *emitted = self.max_length;
        } else {
            *emitted += produced;
        }
    }
}

/// `BertNormalizer { clean_text, handle_chinese_chars, strip_accents: lowercase, lowercase }`.
pub fn normalize(text: &str) -> String {
    // clean_text + handle_chinese_chars
    let mut cleaned = String::with_capacity(text.len() + text.len() / 8);
    for c in text.chars() {
        if c == '\0' || c == '\u{fffd}' || is_control(c) {
            continue;
        }
        let c = if c.is_whitespace() { ' ' } else { c };
        if is_chinese_char(c) {
            cleaned.push(' ');
            cleaned.push(c);
            cleaned.push(' ');
        } else {
            cleaned.push(c);
        }
    }

    // strip_accents (NFD, drop nonspacing marks), then lowercase
    let mut out = String::with_capacity(cleaned.len());
    for c in cleaned
        .chars()
        .nfd()
        .filter(|c| c.general_category() != GeneralCategory::NonspacingMark)
    {
        out.extend(c.to_lowercase());
    }
    out
}

/// Control and other non-printing characters, except tab, newline and carriage return
/// (those are treated as whitespace).
fn is_control(c: char) -> bool {
    if matches!(c, '\t' | '\n' | '\r') {
        return false;
    }
    matches!(
        c.general_category(),
        GeneralCategory::Control
            | GeneralCategory::Format
            | GeneralCategory::Surrogate
            | GeneralCategory::PrivateUse
            | GeneralCategory::Unassigned
    )
}

/// ASCII punctuation/symbols (as BERT does) or any Unicode punctuation.
fn is_bert_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || c.general_category_group() == GeneralCategoryGroup::Punctuation
}

fn is_chinese_char(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF
        | 0x3400..=0x4DBF
        | 0x20000..=0x2A6DF
        | 0x2A700..=0x2B73F
        | 0x2B740..=0x2B81F
        | 0x2B920..=0x2CEAF
        | 0xF900..=0xFAFF
        | 0x2F800..=0x2FA1F)
}
