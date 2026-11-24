//! A `no_std` BERT WordPiece tokenizer.

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
pub(crate) struct Vocab {
    blob: &'static [u8],
}

impl core::fmt::Debug for Vocab {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Vocab")
            .field("blob_bytes", &self.blob.len())
            .finish()
    }
}

impl Vocab {
    /// Wraps `blob`, which must follow the layout described on [`Vocab`].
    pub const fn new(blob: &'static [u8]) -> Self {
        Self { blob }
    }

    /// Returns the number of tokens in the vocabulary.
    ///
    /// # Panics
    ///
    /// Panics if the blob lacks the four-byte token count.
    pub fn len(&self) -> usize {
        read_u32(self.blob, 0) as usize
    }

    fn entry(&self, index: usize) -> (&'static [u8], u32) {
        let off = read_u32(self.blob, 4 + 4 * index) as usize;
        let id = read_u32(self.blob, off);
        let len = self.blob[off + 4] as usize;
        (&self.blob[off + 5..off + 5 + len], id)
    }

    /// Returns the id of `token`, if it is in the vocabulary.
    ///
    /// # Panics
    ///
    /// May panic if the count, offsets, or token entries read during lookup
    /// extend past the blob.
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

/// BERT-style WordPiece tokenizer over a vocabulary that stays in flash.
///
/// It implements one fixed pipeline, the one potion-2M declares in its `tokenizer.json`. The
/// pipeline of a model's `tokenizer.json` is not read, so a model must use the same one:
///
/// 1. `BertNormalizer` (`clean_text`, `handle_chinese_chars`, accent stripping, lowercasing)
/// 2. `BertPreTokenizer` (split on whitespace, isolate every punctuation character)
/// 3. `WordPiece` (greedy longest-match-first, `##` continuation prefix, whole word
///    becomes `[UNK]` if any piece is missing or if it is longer than the word limit)
///
/// No special tokens are added, which is what static (Model2Vec) embedding models expect.
///
/// The vocabulary is **not** parsed or copied: the [`model!`](crate::model!) macro compiles
/// it into a sorted binary blob that is binary-searched where it lives, in flash.
#[derive(Debug, Clone, Copy)]
pub struct WordPieceTokenizer {
    vocab: Vocab,
    unk_id: u32,
    max_chars_per_word: usize,
    max_length: usize,
}

impl WordPieceTokenizer {
    /// Creates a tokenizer over `vocab`.
    ///
    /// * `unk_id`: the id of the `[UNK]` token.
    /// * `max_chars_per_word`: words longer than this (in chars) become `[UNK]`.
    /// * `max_length`: truncation, counted in tokens (including `[UNK]`s), like the
    ///   reference implementation which truncates before `[UNK]`s are removed.
    pub(crate) const fn new(
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

    /// Returns the id of the `[UNK]` token.
    #[must_use]
    pub const fn unk_id(&self) -> u32 {
        self.unk_id
    }

    /// Returns the truncation, in tokens, applied by [`encode`](Self::encode).
    #[must_use]
    pub const fn max_length(&self) -> usize {
        self.max_length
    }

    /// Tokenizes `text` and returns its token ids, **without** the `[UNK]` ids
    /// (a static embedding model ignores them).
    pub fn encode(&self, text: &str) -> Vec<u32> {
        self.encode_text(text, false)
    }

    /// Like [`encode`](Self::encode), but keeps `[UNK]` ids in the output.
    pub fn encode_with_unk(&self, text: &str) -> Vec<u32> {
        self.encode_text(text, true)
    }

    /// Normalizes text, splits it into words, and tokenizes each word.
    ///
    /// Example: "hello, world" -> words "hello" "," "world" -> one `word_piece` call each.
    fn encode_text(&self, text: &str, keep_unk: bool) -> Vec<u32> {
        let mut ids = Vec::new();
        let mut remaining = self.max_length;
        if remaining == 0 {
            return ids;
        }
        let text = normalize(text);
        // Reused buffer for building vocab lookup keys, so we don't allocate per lookup.
        let mut scratch = String::new();
        let mut encode_word = |word: &str| {
            remaining -= self.word_piece(word, &mut scratch, &mut ids, remaining, keep_unk);
            remaining == 0
        };
        // Byte index where the word currently being scanned began (None between words).
        let mut word_start: Option<usize> = None;

        // BertPreTokenizer: whitespace separates words and is dropped; every punctuation
        // character is a word of its own.
        for (i, c) in text.char_indices() {
            let is_space = c.is_whitespace();
            let is_punct = !is_space && is_bert_punctuation(c);
            if is_space || is_punct {
                // A separator ends the word we were scanning, if any.
                if let Some(start) = word_start.take()
                    && encode_word(&text[start..i])
                {
                    return ids;
                }
                // Punctuation is not just a separator: it is also a one-char word itself.
                if is_punct && encode_word(&text[i..i + c.len_utf8()]) {
                    return ids;
                }
            } else if word_start.is_none() {
                // First ordinary char after a separator: a new word starts here.
                word_start = Some(i);
            }
        }
        // The text may end in the middle of a word (no trailing separator): flush it.
        if let Some(start) = word_start {
            encode_word(&text[start..]);
        }
        ids
    }

    /// Greedy longest-match-first WordPiece over one pre-tokenized word.
    ///
    /// Repeatedly take the longest prefix of the remaining text that is in the vocab.
    /// Pieces after the first are looked up with a `##` prefix. Example, with
    /// "unaffable" and vocab {"un", "##aff", "##able"}:
    /// try "unaffable".."una" (miss) -> "un" (hit); then "##affable".."##af" (miss)
    /// -> "##aff" (hit); then "##able" (hit) => [un, ##aff, ##able].
    /// If at any point not even a single char matches, the *whole word* becomes [UNK].
    /// Returns how many tokens count toward truncation, including filtered unknown IDs.
    fn word_piece(
        &self,
        word: &str,
        scratch: &mut String,
        ids: &mut Vec<u32>,
        remaining: usize,
        keep_unk: bool,
    ) -> usize {
        let mut consumed = 0;
        // Where this word's ids begin in `ids`, so we can roll them back on failure.
        let first_id = ids.len();
        // Too-long words are [UNK] without even trying.
        let mut ok = word.chars().count() <= self.max_chars_per_word;

        // `start..end` is the candidate piece (byte offsets into `word`).
        let mut start = 0;
        while ok && start < word.len() {
            // The blob encodes the complete vocabulary key length in one byte.
            let prefix = if start == 0 { "" } else { CONTINUING_PREFIX };
            let max_bytes = usize::from(u8::MAX) - prefix.len();
            let mut end = word.floor_char_boundary(start.saturating_add(max_bytes));
            let mut found = None;
            while end > start {
                scratch.clear();
                scratch.push_str(prefix);
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
                    if consumed < remaining {
                        consumed += 1;
                        if keep_unk || id != self.unk_id {
                            ids.push(id);
                        }
                    }
                    start = end;
                }
                // Nothing matched, not even one char: give up on the whole word.
                None => ok = false,
            }
        }

        if !ok {
            // The whole word is unknown, not just the piece that failed.
            ids.truncate(first_id);
            if keep_unk {
                ids.push(self.unk_id);
            }
            return 1;
        }
        consumed
    }
}

/// Normalizes text for BERT WordPiece tokenization.
///
/// Cleans control characters and whitespace, pads Chinese characters with spaces,
/// lowercases letters, and strips accents.
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

#[cfg(test)]
mod tests {
    use super::{Vocab, WordPieceTokenizer, normalize};
    use crate::test_util::{VOCAB, tokenizer, vocab};

    #[test]
    fn normalize_applies_bert_text_rules() {
        for (rule, input, expected) in [
            ("strip accents and lowercase", "Héllo WÖRLD", "hello world"),
            ("replace whitespace", "a\tb\nc\rd", "a b c d"),
            (
                "remove control and format characters",
                "a\u{0}b\u{fffd}c\u{200b}d",
                "abcd",
            ),
            ("pad CJK characters", "中文", " 中  文 "),
        ] {
            assert_eq!(normalize(input), expected, "{rule}");
        }
    }

    #[test]
    fn debug_summarizes_an_incomplete_vocab_without_reading_its_header() {
        let vocab = Vocab::new(&[173, 201, 239]);
        let debug = alloc::format!("{vocab:?}");
        assert!(debug.contains("blob_bytes: 3"));
        assert!(!debug.contains("173, 201, 239"));
    }

    #[test]
    fn vocab_finds_present_tokens_and_rejects_missing_ones() {
        let vocab = vocab(VOCAB);
        assert_eq!(vocab.get("##aff"), Some(2));
        assert_eq!(vocab.get("中"), Some(8));
        assert_eq!(vocab.get("aff"), None);
        assert_eq!(vocab.get(""), None);
        assert_eq!(vocab.len(), 10);
    }

    #[test]
    fn vocab_reports_empty() {
        assert_eq!(vocab(&[]).len(), 0);
    }

    #[test]
    fn encode_prefers_the_longest_initial_and_continuation_pieces() {
        let vocab = vocab(&[
            ("a", 1),
            ("ab", 2),
            ("##b", 3),
            ("##c", 4),
            ("##cd", 5),
            ("##d", 6),
        ]);
        let tokenizer = WordPieceTokenizer::new(vocab, 0, 100, 512);
        assert_eq!(tokenizer.encode("abcd"), [2, 5]);
    }

    #[test]
    fn long_words_fit_the_lookup_buffer_and_validate_the_suffix() {
        let piece = "α".repeat(126);
        let continuation = alloc::format!("##{piece}");
        let tokenizer =
            WordPieceTokenizer::new(vocab(&[(&piece, 1), (&continuation, 2)]), 0, 2000, 1);
        let word = piece.repeat(10);
        let mut scratch = alloc::string::String::with_capacity(255);
        let mut ids = alloc::vec::Vec::new();
        tokenizer.word_piece(&word, &mut scratch, &mut ids, 1, true);
        assert_eq!(ids, [1]);
        assert_eq!(scratch.capacity(), 255);
        assert_eq!(tokenizer.encode_with_unk(&alloc::format!("{word}x")), [0]);
    }

    #[test]
    fn encode_isolates_punctuation() {
        let tokenizer = tokenizer(100, 512);
        assert_eq!(tokenizer.encode("Hello, world"), [4, 5, 6]);
        assert_eq!(tokenizer.encode("hello,world"), [4, 5, 6]);
    }

    #[test]
    fn encode_strips_accents_and_splits_chinese_characters() {
        assert_eq!(tokenizer(100, 512).encode("Héllo中文"), [4, 8, 9]);
    }

    #[test]
    fn encode_drops_unknown_words() {
        assert_eq!(tokenizer(100, 512).encode("hello xyz world"), [4, 6]);
    }

    #[test]
    fn directly_matched_unknown_ids_count_toward_truncation() {
        let vocab = vocab(&[("unknown", 3), ("known", 0)]);
        let tokenizer = WordPieceTokenizer::new(vocab, 3, 100, 2);
        assert_eq!(tokenizer.encode("unknown known known"), [0]);
        assert_eq!(tokenizer.encode_with_unk("unknown known known"), [3, 0]);
    }

    #[test]
    fn filtered_unknown_ids_take_no_output_space() {
        let vocab = vocab(&[("unknown", 3)]);
        let tokenizer = WordPieceTokenizer::new(vocab, 3, 100, 4096);
        let text = "missing unknown ".repeat(2048);
        assert_eq!(tokenizer.encode(&text).capacity(), 0);
    }

    #[test]
    fn encode_with_unk_keeps_unknown_words() {
        let tokenizer =
            WordPieceTokenizer::new(vocab(&[("[UNK]", 42), ("hello", 4)]), 42, 100, 512);
        assert_eq!(tokenizer.unk_id(), 42);
        assert_eq!(tokenizer.encode_with_unk("hello xyz hello"), [4, 42, 4]);
    }

    #[test]
    fn partial_match_makes_the_whole_word_unknown() {
        let tokenizer = tokenizer(100, 2);
        assert_eq!(tokenizer.encode("unaffablex hello"), [4]);
        assert_eq!(tokenizer.encode_with_unk("unaffablex hello"), [0, 4]);
    }

    #[test]
    fn long_word_limit_counts_chars_not_bytes() {
        let tokenizer = tokenizer(2, 512);
        assert_eq!(tokenizer.encode_with_unk("中文 un"), [8, 9, 1]);
        assert_eq!(tokenizer.encode_with_unk("unaffable"), [0]);
    }

    #[test]
    fn unknown_multibyte_piece_makes_the_whole_word_unknown() {
        assert_eq!(tokenizer(100, 512).encode_with_unk("un🙂"), [0]);
    }

    #[test]
    fn encode_truncates_inside_a_word() {
        assert_eq!(tokenizer(100, 2).encode("unaffable"), [1, 2]);
    }

    #[test]
    fn unknown_words_count_toward_truncation() {
        let tokenizer = tokenizer(100, 2);
        assert_eq!(tokenizer.encode("xyz hello world"), [4]);
        assert_eq!(tokenizer.encode_with_unk("xyz hello world"), [0, 4]);
    }

    #[test]
    fn blank_text_encodes_to_nothing() {
        let tokenizer = tokenizer(100, 512);
        assert_eq!(tokenizer.encode(""), [0u32; 0]);
        assert_eq!(tokenizer.encode(" \t\n"), [0u32; 0]);
    }
}
