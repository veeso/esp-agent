//! Compiles a WordPiece model folder into the pieces the `model!` expansion embeds.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};
use serde_json::Value;

/// The WordPiece default for `max_input_chars_per_word` in Hugging Face tokenizers.
pub(crate) const DEFAULT_MAX_CHARS_PER_WORD: usize = 100;

/// File name of the tokenizer definition inside a model folder.
const TOKENIZER_FILE: &str = "tokenizer.json";

/// File name of the embedding table inside a model folder.
const MODEL_FILE: &str = "model.safetensors";

/// The only continuation prefix the run-time tokenizer understands.
const CONTINUING_PREFIX: &str = "##";

/// Bytes in the leading `u64` that holds the safetensors header length.
const LENGTH_PREFIX_BYTES: usize = 8;

/// Bytes in one `f32`.
const F32_BYTES: usize = 4;

/// Everything the `model!` expansion needs to know about a model folder.
#[derive(Debug)]
pub(crate) struct CompiledModel {
    /// Path of `<dir>/model.safetensors`.
    pub(crate) model_path: PathBuf,
    /// Path of `<dir>/tokenizer.json`.
    pub(crate) tokenizer_path: PathBuf,
    /// Sorted, offset-indexed vocabulary: `u32 count`, `count x u32` entry offsets
    /// ordered by token bytes, then entries of `u32 id`, `u8 len`, token bytes.
    /// All little-endian.
    pub(crate) vocab_blob: Vec<u8>,
    /// Rows of the embedding table.
    pub(crate) vocab_size: usize,
    /// Columns of the embedding table.
    pub(crate) dim: usize,
    /// Byte offset of the table inside `model.safetensors`.
    pub(crate) tensor_start: usize,
    /// Byte offset one past the end of the table inside `model.safetensors`.
    pub(crate) tensor_end: usize,
    /// Id of the unknown token.
    pub(crate) unk_id: u32,
    /// Words longer than this many chars become the unknown token.
    pub(crate) max_chars_per_word: usize,
}

/// Why a model folder cannot be compiled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CompileError {
    /// A file could not be read.
    Read { path: PathBuf, reason: String },
    /// A model file contains invalid JSON.
    InvalidJson { path: PathBuf, reason: String },
    /// The JSON document does not match the tokenizer schema.
    InvalidTokenizer { reason: String },
    /// The tokenizer is not a WordPiece tokenizer.
    UnsupportedTokenizer { found: String },
    /// The continuation prefix is not `##`.
    UnsupportedPrefix { found: String },
    /// A required tokenizer field is missing or has the wrong type.
    MissingField { field: &'static str },
    /// The word limit cannot be represented on the target.
    InvalidWordLimit { found: Value },
    /// The unknown token is not part of the vocabulary.
    UnknownUnkToken { token: String },
    /// A token does not fit the one-byte length of a vocabulary entry.
    TokenTooLong { token: String },
    /// The encoded vocabulary exceeds its 32-bit offset representation.
    VocabTooLarge,
    /// The safetensors header is truncated or malformed.
    InvalidHeader,
    /// The JSON document does not match the safetensors header schema.
    InvalidHeaderFields { reason: String },
    /// Safetensors metadata must contain string values.
    InvalidMetadata,
    /// The safetensors file holds no tensor.
    MissingTensor,
    /// More than one tensor was declared.
    ExtraTensor { first: String, second: String },
    /// The offsets do not describe the entire payload.
    InvalidOffsets { found: Value, bytes: usize },
    /// A required tensor field is missing.
    MissingTensorField { field: &'static str },
    /// The table is not made of `f32` values.
    UnsupportedDtype { found: Value },
    /// The table is not a two-dimensional matrix.
    InvalidShape { found: Value },
    /// The byte range of the table disagrees with its shape.
    SizeMismatch { expected: usize, found: usize },
    /// An embedding weight is NaN or infinite.
    NonFiniteWeight { row: usize, column: usize },
    /// A vocabulary id is not an unsigned 32-bit integer.
    InvalidId { token: String, found: Value },
    /// Two tokens use the same table row.
    DuplicateId { token: String, id: u32 },
    /// Some rows have no token.
    RowCount { tokens: usize, rows: usize },
    /// A vocabulary id points outside the table.
    IdOutOfRange { token: String, id: u32, rows: usize },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, reason } => {
                write!(f, "cannot read {path}: {reason}", path = path.display())
            }
            Self::InvalidJson { path, reason } => {
                write!(f, "{path}: invalid JSON: {reason}", path = path.display())
            }
            Self::InvalidTokenizer { reason } => {
                write!(f, "{TOKENIZER_FILE}: invalid tokenizer: {reason}")
            }
            Self::UnsupportedTokenizer { found } => write!(
                f,
                "{TOKENIZER_FILE}: only WordPiece tokenizers are supported, found {found}"
            ),
            Self::UnsupportedPrefix { found } => write!(
                f,
                "{TOKENIZER_FILE}: the continuing subword prefix must be `{CONTINUING_PREFIX}`, found `{found}`"
            ),
            Self::MissingField { field } => {
                write!(f, "{TOKENIZER_FILE}: `{field}` is missing or invalid")
            }
            Self::InvalidWordLimit { found } => write!(
                f,
                "{TOKENIZER_FILE}: max_input_chars_per_word must be an integer from 0 to 4294967295, found {found}"
            ),
            Self::ExtraTensor { first, second } => write!(
                f,
                "{MODEL_FILE}: expected one embedding tensor, found both `{first}` and `{second}`"
            ),
            Self::InvalidOffsets { found, bytes } => write!(
                f,
                "{MODEL_FILE}: data_offsets must cover the entire payload as [0, {bytes}], found {found}"
            ),
            Self::InvalidId { token, found } => write!(
                f,
                "{TOKENIZER_FILE}: the id for token `{token}` must be an integer from 0 to {max}, found {found}",
                max = u32::MAX
            ),
            Self::DuplicateId { token, id } => write!(
                f,
                "{TOKENIZER_FILE}: the token `{token}` repeats vocabulary id {id}"
            ),
            Self::RowCount { tokens, rows } => write!(
                f,
                "{TOKENIZER_FILE}: the vocabulary has {tokens} tokens but the table has {rows} rows"
            ),
            Self::UnknownUnkToken { token } => write!(
                f,
                "{TOKENIZER_FILE}: the unk_token `{token}` is not in the vocabulary"
            ),
            Self::TokenTooLong { token } => write!(
                f,
                "{TOKENIZER_FILE}: the token `{token}` is longer than {max} bytes",
                max = u8::MAX
            ),
            Self::VocabTooLarge => write!(
                f,
                "{TOKENIZER_FILE}: the encoded vocabulary exceeds the 32-bit offset limit"
            ),
            Self::InvalidHeader => {
                write!(f, "{MODEL_FILE}: the header is truncated or invalid")
            }
            Self::InvalidHeaderFields { reason } => {
                write!(f, "{MODEL_FILE}: invalid header: {reason}")
            }
            Self::InvalidMetadata => write!(
                f,
                "{MODEL_FILE}: `__metadata__` must map strings to strings"
            ),
            Self::MissingTensor => write!(f, "{MODEL_FILE}: the file holds no tensor"),
            Self::MissingTensorField { field } => {
                write!(f, "{MODEL_FILE}: missing tensor field `{field}`")
            }
            Self::UnsupportedDtype { found } => {
                write!(f, "{MODEL_FILE}: dtype must be \"F32\", found {found}")
            }
            Self::InvalidShape { found } => write!(
                f,
                "{MODEL_FILE}: the table must be a nonempty two-dimensional matrix, found shape {found}"
            ),
            Self::SizeMismatch { expected, found } => write!(
                f,
                "{MODEL_FILE}: the table spans {found} bytes but its shape needs {expected}"
            ),
            Self::NonFiniteWeight { row, column } => write!(
                f,
                "{MODEL_FILE}: non-finite weight at row {row}, column {column}"
            ),
            Self::IdOutOfRange { token, id, rows } => write!(
                f,
                "{TOKENIZER_FILE}: the token `{token}` has id {id} but the table has {rows} rows"
            ),
        }
    }
}

/// Reads and validates the model folder at `dir`.
///
/// `dir` must hold `tokenizer.json` (a WordPiece tokenizer) and `model.safetensors`
/// (one `f32` matrix with a row per vocabulary id).
pub(crate) fn compile(dir: &Path) -> Result<CompiledModel, CompileError> {
    let tokenizer_path = dir.join(TOKENIZER_FILE);
    let model_path = dir.join(MODEL_FILE);

    let (vocab, unk_id, max_chars_per_word) = read_tokenizer(&tokenizer_path)?;
    let table = read_table(&model_path)?;

    let mut seen = vec![false; table.rows];
    for (token, id) in &vocab {
        let Some(used) = seen.get_mut(*id as usize) else {
            return Err(CompileError::IdOutOfRange {
                token: token.clone(),
                id: *id,
                rows: table.rows,
            });
        };
        if *used {
            return Err(CompileError::DuplicateId {
                token: token.clone(),
                id: *id,
            });
        }
        *used = true;
    }
    if vocab.len() != table.rows {
        return Err(CompileError::RowCount {
            tokens: vocab.len(),
            rows: table.rows,
        });
    }

    Ok(CompiledModel {
        model_path,
        tokenizer_path,
        vocab_blob: vocab_blob(&vocab)?,
        vocab_size: table.rows,
        dim: table.dim,
        tensor_start: table.start,
        tensor_end: table.end,
        unk_id,
        max_chars_per_word,
    })
}

fn read(path: &Path) -> Result<Vec<u8>, CompileError> {
    fs::read(path).map_err(|error| CompileError::Read {
        path: path.to_owned(),
        reason: error.to_string(),
    })
}

/// Renders a JSON value for an error message, without quotes around strings.
fn describe(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

type Vocab = Vec<(String, u32)>;

#[derive(serde::Deserialize)]
struct TokenizerFile {
    model: Option<TokenizerModel>,
}

#[derive(serde::Deserialize)]
struct TokenizerModel {
    #[serde(default, deserialize_with = "deserialize_vocab")]
    vocab: Option<UniqueFields<Value>>,
    #[serde(flatten)]
    settings: UniqueFields<Value>,
}

fn deserialize_vocab<'de, D>(deserializer: D) -> Result<Option<UniqueFields<Value>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::deserialize(deserializer)
        .map_err(|error| D::Error::custom(format!("model.vocab: {error}")))
}

/// Returns the vocabulary sorted by token bytes, the unknown id, and the word limit.
fn read_tokenizer(path: &Path) -> Result<(Vocab, u32, usize), CompileError> {
    let bytes = read(path)?;
    let source = std::str::from_utf8(&bytes).map_err(|error| CompileError::InvalidJson {
        path: path.to_owned(),
        reason: format!(
            "invalid UTF-8 at byte {offset}",
            offset = error.valid_up_to()
        ),
    })?;
    let json: TokenizerFile = serde_json::from_str(source).map_err(|error| {
        // Field visitors can report a schema error before reaching a syntax error.
        match serde_json::from_str::<Value>(source) {
            Ok(_) => CompileError::InvalidTokenizer {
                reason: error.to_string(),
            },
            Err(syntax) => CompileError::InvalidJson {
                path: path.to_owned(),
                reason: syntax.to_string(),
            },
        }
    })?;
    // Serde also accepts positional arrays for derived structs.
    if !source.trim_ascii_start().starts_with('{') {
        return Err(CompileError::InvalidTokenizer {
            reason: "expected a tokenizer object with a named `model` field".into(),
        });
    }
    let TokenizerModel { vocab, settings } = json
        .model
        .ok_or(CompileError::MissingField { field: "model" })?;
    let model = Value::Object(settings.0.into_iter().collect());
    if model["type"] != "WordPiece" {
        return Err(CompileError::UnsupportedTokenizer {
            found: describe(&model["type"]),
        });
    }
    if model["continuing_subword_prefix"] != CONTINUING_PREFIX {
        return Err(CompileError::UnsupportedPrefix {
            found: describe(&model["continuing_subword_prefix"]),
        });
    }
    let unk_token = model["unk_token"]
        .as_str()
        .ok_or(CompileError::MissingField { field: "unk_token" })?;
    let max_chars_per_word = match model.get("max_input_chars_per_word") {
        None => DEFAULT_MAX_CHARS_PER_WORD,
        Some(value) => value
            .as_u64()
            .and_then(|limit| u32::try_from(limit).ok())
            .ok_or_else(|| CompileError::InvalidWordLimit {
                found: value.clone(),
            })? as usize,
    };

    let UniqueFields(tokens) = vocab.ok_or(CompileError::MissingField { field: "vocab" })?;
    let vocab = tokens
        .into_iter()
        .map(|(token, id)| {
            let Some(id) = id.as_u64().and_then(|id| u32::try_from(id).ok()) else {
                return Err(CompileError::InvalidId { token, found: id });
            };
            Ok((token, id))
        })
        .collect::<Result<Vocab, _>>()?;

    if let Some((token, _)) = vocab
        .iter()
        .find(|(token, _)| token.len() > usize::from(u8::MAX))
    {
        return Err(CompileError::TokenTooLong {
            token: token.clone(),
        });
    }
    let unk_id = vocab
        .iter()
        .find(|(token, _)| token == unk_token)
        .map(|(_, id)| *id)
        .ok_or_else(|| CompileError::UnknownUnkToken {
            token: unk_token.to_owned(),
        })?;
    Ok((vocab, unk_id, max_chars_per_word))
}

/// Accounts for the count, each offset, ID and length byte, and the token bytes.
fn vocab_blob_size(count: usize, token_bytes: usize) -> Result<usize, CompileError> {
    count
        .checked_mul(9)
        .and_then(|bytes| bytes.checked_add(4))
        .and_then(|bytes| bytes.checked_add(token_bytes))
        .filter(|&bytes| u32::try_from(bytes).is_ok())
        .ok_or(CompileError::VocabTooLarge)
}

/// Encodes the sorted `vocab` in the layout the run-time `Vocab` reads.
fn vocab_blob(vocab: &[(String, u32)]) -> Result<Vec<u8>, CompileError> {
    let size = vocab_blob_size(
        vocab.len(),
        vocab.iter().map(|(token, _)| token.len()).sum(),
    )?;
    let header = 4 + 4 * vocab.len();
    let mut blob = Vec::with_capacity(size);
    let count = u32::try_from(vocab.len()).expect("the vocabulary fits in 4 GiB");
    blob.extend_from_slice(&count.to_le_bytes());
    blob.resize(header, 0);
    for (index, (token, id)) in vocab.iter().enumerate() {
        let offset = u32::try_from(blob.len()).expect("the vocabulary fits in 4 GiB");
        let slot = 4 + 4 * index;
        blob[slot..slot + 4].copy_from_slice(&offset.to_le_bytes());
        blob.extend_from_slice(&id.to_le_bytes());
        blob.push(u8::try_from(token.len()).expect("token lengths were checked"));
        blob.extend_from_slice(token.as_bytes());
    }
    Ok(blob)
}

/// Location and shape of the embedding table inside `model.safetensors`.
struct Table {
    rows: usize,
    dim: usize,
    start: usize,
    end: usize,
}

/// Deserializes an object without silently overwriting repeated keys.
struct UniqueFields<T>(BTreeMap<String, T>);

impl<'de, T> Deserialize<'de> for UniqueFields<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FieldsVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for FieldsVisitor<T>
        where
            T: Deserialize<'de>,
        {
            type Value = UniqueFields<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut fields = BTreeMap::new();
                while let Some(name) = map.next_key::<String>()? {
                    let entry = match fields.entry(name) {
                        Entry::Occupied(entry) => {
                            let name = entry.key();
                            return Err(A::Error::custom(format!("duplicate field `{name}`")));
                        }
                        Entry::Vacant(entry) => entry,
                    };
                    let name = entry.key();
                    let value = map.next_value().map_err(|error| {
                        A::Error::custom(format!("invalid field `{name}`: {error}"))
                    })?;
                    entry.insert(value);
                }
                Ok(UniqueFields(fields))
            }
        }

        deserializer.deserialize_map(FieldsVisitor(PhantomData))
    }
}

fn read_table(path: &Path) -> Result<Table, CompileError> {
    let file = read(path)?;
    let length_bytes = file
        .first_chunk::<LENGTH_PREFIX_BYTES>()
        .ok_or(CompileError::InvalidHeader)?;
    let header_len = usize::try_from(u64::from_le_bytes(*length_bytes))
        .map_err(|_| CompileError::InvalidHeader)?;
    let data_start = LENGTH_PREFIX_BYTES
        .checked_add(header_len)
        .filter(|end| *end <= file.len())
        .ok_or(CompileError::InvalidHeader)?;
    let bytes = &file[LENGTH_PREFIX_BYTES..data_start];
    if bytes.first() != Some(&b'{') {
        return Err(CompileError::InvalidHeader);
    }
    let UniqueFields(mut header): UniqueFields<UniqueFields<Value>> = serde_json::from_slice(bytes)
        .map_err(|error| match serde_json::from_slice::<Value>(bytes) {
            Ok(_) => CompileError::InvalidHeaderFields {
                reason: error.to_string(),
            },
            Err(syntax) => CompileError::InvalidJson {
                path: path.to_owned(),
                reason: syntax.to_string(),
            },
        })?;
    if let Some(UniqueFields(metadata)) = header.remove("__metadata__")
        && metadata.values().any(|value| !value.is_string())
    {
        return Err(CompileError::InvalidMetadata);
    }
    let mut tensors = header.iter();
    let (first, UniqueFields(tensor)) = tensors.next().ok_or(CompileError::MissingTensor)?;
    if let Some((second, _)) = tensors.next() {
        return Err(CompileError::ExtraTensor {
            first: first.clone(),
            second: second.clone(),
        });
    }

    let dtype = tensor
        .get("dtype")
        .ok_or(CompileError::MissingTensorField { field: "dtype" })?;
    if dtype.as_str() != Some("F32") {
        return Err(CompileError::UnsupportedDtype {
            found: dtype.clone(),
        });
    }
    let shape = tensor
        .get("shape")
        .ok_or(CompileError::MissingTensorField { field: "shape" })?;
    let invalid_shape = || CompileError::InvalidShape {
        found: shape.clone(),
    };
    let Some([rows, dim]) = shape.as_array().map(Vec::as_slice) else {
        return Err(invalid_shape());
    };
    let dimension = |value: &Value| {
        value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n > 0)
            .ok_or_else(invalid_shape)
    };
    let (rows, dim) = (dimension(rows)?, dimension(dim)?);

    let offsets = tensor.get("data_offsets").unwrap_or(&Value::Null);
    let payload_len = file.len() - data_start;
    let invalid_offsets = || CompileError::InvalidOffsets {
        found: offsets.clone(),
        bytes: payload_len,
    };
    let Some([start, end]) = offsets.as_array().map(Vec::as_slice) else {
        return Err(invalid_offsets());
    };
    if start.as_u64() != Some(0) {
        return Err(invalid_offsets());
    }
    let length = end
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(invalid_offsets)?;
    let expected = rows
        .checked_mul(dim)
        .and_then(|cells| cells.checked_mul(F32_BYTES))
        .ok_or_else(invalid_shape)?;
    if length != expected {
        return Err(CompileError::SizeMismatch {
            expected,
            found: length,
        });
    }
    if length != payload_len {
        return Err(invalid_offsets());
    }
    if let Some(index) = file[data_start..]
        .as_chunks::<F32_BYTES>()
        .0
        .iter()
        .position(|bytes| !f32::from_le_bytes(*bytes).is_finite())
    {
        return Err(CompileError::NonFiniteWeight {
            row: index / dim,
            column: index % dim,
        });
    }
    Ok(Table {
        rows,
        dim,
        start: data_start,
        end: file.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};

    use super::{CompileError, CompiledModel, compile};
    use crate::test_util::{valid, write_model};

    fn compile_valid(
        name: &str,
        edit: impl FnOnce(&mut Value, &mut Value, &mut Vec<u8>),
    ) -> Result<CompiledModel, CompileError> {
        let (mut tokenizer, mut header, mut data) = valid();
        edit(&mut tokenizer, &mut header, &mut data);
        compile(write_model(name, tokenizer, header, &data).path())
    }

    #[test]
    fn compiles_a_valid_model() {
        let (tokenizer, header, data) = valid();
        let header_len = header.to_string().len();
        let model = compile(write_model("valid", tokenizer, header, &data).path())
            .expect("the base case compiles");

        assert_eq!(model.vocab_size, 2);
        assert_eq!(model.dim, 2);
        assert_eq!(model.unk_id, 0);
        assert_eq!(model.max_chars_per_word, 100);
        assert_eq!(model.tensor_start, 8 + header_len);
        assert_eq!(model.tensor_end, model.tensor_start + 16);
        assert_eq!(
            model.vocab_blob,
            [
                2, 0, 0, 0, // count
                12, 0, 0, 0, 22, 0, 0, 0, // offsets
                0, 0, 0, 0, 5, b'[', b'U', b'N', b'K', b']', // [UNK] = 0
                1, 0, 0, 0, 2, b'h', b'i', // hi = 1
            ]
        );
    }

    #[test]
    fn reads_max_input_chars_per_word() {
        for limit in [0, 7, u32::MAX as usize] {
            let model = compile_valid("max-chars", |tokenizer, _, _| {
                tokenizer["model"]["max_input_chars_per_word"] = json!(limit);
            })
            .expect("the model compiles");
            assert_eq!(model.max_chars_per_word, limit);
        }
    }

    #[test]
    fn vocab_blob_sorts_by_bytes() {
        let (tokenizer, mut header, _) = valid();
        header["embeddings"]["shape"] = json!([3, 2]);
        header["embeddings"]["data_offsets"] = json!([0, 24]);
        let dir = write_model("sorted", tokenizer, header, &[0; 24]);
        let raw = r###"{"model":{"type":"WordPiece","continuing_subword_prefix":"##","unk_token":"[UNK]","vocab":{"é":1,"z":2,"[UNK]":0}}}"###;
        fs::write(dir.path().join("tokenizer.json"), raw).unwrap();
        let model = compile(dir.path()).expect("the model compiles");

        let mut tokens: Vec<&[u8]> = Vec::new();
        let mut at = 16;
        while at < model.vocab_blob.len() {
            let len = usize::from(model.vocab_blob[at + 4]);
            tokens.push(&model.vocab_blob[at + 5..at + 5 + len]);
            at += 5 + len;
        }
        assert_eq!(tokens, [b"[UNK]".as_slice(), b"z", "é".as_bytes()]);
    }

    #[test]
    fn names_each_missing_model_file() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("model");
        let error = compile(&dir)
            .expect_err("the folder does not exist")
            .to_string();
        assert!(
            error.contains(&dir.join("tokenizer.json").display().to_string()),
            "{error}"
        );

        fs::create_dir(&dir).unwrap();
        let (tokenizer, _, _) = valid();
        fs::write(dir.join("tokenizer.json"), tokenizer.to_string()).unwrap();
        let error = compile(&dir)
            .expect_err("the weights file is missing")
            .to_string();
        assert!(
            error.contains(&dir.join("model.safetensors").display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn rejects_invalid_tokenizer_json() {
        let dir = {
            let (tokenizer, header, data) = valid();
            write_model("bad-json", tokenizer, header, &data)
        };
        fs::write(dir.path().join("tokenizer.json"), "not json")
            .expect("tokenizer.json can be written");
        let error = compile(dir.path()).expect_err("the tokenizer is not JSON");
        assert!(matches!(error, CompileError::InvalidJson { .. }));
    }

    #[test]
    fn rejects_an_array_at_the_tokenizer_root() {
        let message = compile_valid("tokenizer-array", |tokenizer, _, _| {
            *tokenizer = json!([tokenizer["model"]]);
        })
        .expect_err("a tokenizer must have a named model field")
        .to_string();
        assert!(message.contains("tokenizer.json"), "{message}");
        assert!(message.contains("object"), "{message}");
    }

    #[test]
    fn rejects_invalid_utf8_in_an_ignored_field() {
        let (tokenizer, header, data) = valid();
        let dir = write_model("invalid-utf8", tokenizer.clone(), header, &data);
        let mut bytes = br#"{"description":""#.to_vec();
        let offset = bytes.len();
        bytes.extend_from_slice(b"\xff\",");
        bytes.extend_from_slice(&tokenizer.to_string().as_bytes()[1..]);
        fs::write(dir.path().join("tokenizer.json"), bytes).unwrap();
        let message = compile(dir.path()).unwrap_err().to_string();
        assert!(message.contains("tokenizer.json"), "{message}");
        assert!(
            message.contains(&format!("UTF-8 at byte {offset}")),
            "{message}"
        );
    }

    #[test]
    fn identifies_vocab_schema_errors() {
        let message = compile_valid("vocab-array", |tokenizer, _, _| {
            tokenizer["model"]["vocab"] = json!([]);
        })
        .unwrap_err()
        .to_string();
        for expected in [
            "tokenizer.json",
            "invalid tokenizer",
            "model.vocab",
            "object",
        ] {
            assert!(message.contains(expected), "{message}");
        }
    }

    #[test]
    fn rejects_repeated_vocabulary_tokens() {
        let (tokenizer, header, data) = valid();
        let dir = write_model("repeated-token", tokenizer, header, &data);
        for (first, second) in [(0, 1), (1, 0)] {
            let raw = format!(
                r###"{{"model":{{"type":"WordPiece","continuing_subword_prefix":"##","unk_token":"[UNK]","vocab":{{"[UNK]":0,"hello":{first},"hello":{second}}}}}}}"###
            );
            fs::write(dir.path().join("tokenizer.json"), raw).unwrap();
            let message = compile(dir.path()).unwrap_err().to_string();
            assert!(message.contains("tokenizer.json"), "{message}");
            assert!(message.contains("duplicate field `hello`"), "{message}");
        }
    }

    #[test]
    fn rejects_repeated_tokenizer_settings_in_either_order() {
        let (tokenizer, header, data) = valid();
        let dir = write_model("repeated-setting", tokenizer.clone(), header, &data);
        for (first, second) in [("BPE", "WordPiece"), ("WordPiece", "BPE")] {
            let raw = tokenizer.to_string().replace(
                r#""type":"WordPiece""#,
                &format!(r#""type":"{first}","type":"{second}""#),
            );
            fs::write(dir.path().join("tokenizer.json"), raw).unwrap();
            let message = compile(dir.path()).unwrap_err().to_string();
            assert!(message.contains("tokenizer.json"), "{message}");
            assert!(message.contains("duplicate field `type`"), "{message}");
        }
    }

    #[test]
    fn rejects_non_wordpiece_tokenizers() {
        let error = compile_valid("bpe", |tokenizer, _, _| {
            tokenizer["model"]["type"] = json!("BPE");
        })
        .expect_err("BPE is unsupported");
        assert_eq!(
            error,
            CompileError::UnsupportedTokenizer {
                found: "BPE".into()
            }
        );
    }

    #[test]
    fn rejects_other_continuation_prefixes() {
        let error = compile_valid("prefix", |tokenizer, _, _| {
            tokenizer["model"]["continuing_subword_prefix"] = json!("@@");
        })
        .expect_err("only ## is supported");
        assert_eq!(
            error,
            CompileError::UnsupportedPrefix { found: "@@".into() }
        );
    }

    #[test]
    fn identifies_missing_tokenizer_fields() {
        for (parent, field) in [("", "model"), ("/model", "unk_token"), ("/model", "vocab")] {
            let error = compile_valid("missing-field", |tokenizer, _, _| {
                tokenizer
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(field);
            })
            .unwrap_err();
            assert_eq!(error, CompileError::MissingField { field });
            assert_eq!(
                error.to_string(),
                format!("tokenizer.json: `{field}` is missing or invalid")
            );
        }
    }

    #[test]
    fn rejects_an_unk_token_outside_the_vocab() {
        let error = compile_valid("unk", |tokenizer, _, _| {
            tokenizer["model"]["unk_token"] = json!("<unk>");
        })
        .expect_err("<unk> is not in the vocab");
        assert_eq!(
            error,
            CompileError::UnknownUnkToken {
                token: "<unk>".into()
            }
        );
    }

    #[test]
    fn checks_the_encoded_vocabulary_size_without_allocating() {
        let max = u32::MAX as usize;
        assert_eq!(super::vocab_blob_size(1, max - 13), Ok(max));
        for (count, token_bytes) in [(1, max - 12), (usize::MAX, 0), (1, usize::MAX)] {
            let message = super::vocab_blob_size(count, token_bytes)
                .unwrap_err()
                .to_string();
            assert!(
                message.contains("tokenizer.json") && message.contains("32-bit"),
                "{message}"
            );
        }
    }

    #[test]
    fn enforces_the_255_byte_token_limit() {
        let compile_token = |token: &str| {
            compile_valid("long-token", |tokenizer, _, _| {
                tokenizer["model"]["vocab"] = json!({"[UNK]": 0, (token): 1});
            })
        };
        let token = "a".repeat(255);
        let model = compile_token(&token).expect("a 255-byte token fits");
        // The second entry follows the 12-byte header and the 10-byte [UNK] entry.
        let entry = &model.vocab_blob[22..];
        assert_eq!(entry[..5], [1, 0, 0, 0, 255]);
        assert_eq!(&entry[5..], token.as_bytes());

        let oversized = "a".repeat(256);
        let error = compile_token(&oversized).expect_err("a 256-byte token exceeds the limit");
        assert_eq!(error, CompileError::TokenTooLong { token: oversized });
    }

    #[test]
    fn rejects_a_truncated_file() {
        let (tokenizer, header, data) = valid();
        let dir = write_model("truncated", tokenizer, header, &data);
        fs::write(dir.path().join("model.safetensors"), [0u8; 4]).expect("the file can be written");
        assert_eq!(
            compile(dir.path()).expect_err("the file is truncated"),
            CompileError::InvalidHeader
        );
    }

    #[test]
    fn rejects_a_file_without_tensors() {
        let error = compile_valid("no-tensor", |_, header, _| {
            *header = json!({"__metadata__": {}});
        })
        .expect_err("there is no tensor");
        assert_eq!(error, CompileError::MissingTensor);
    }

    #[test]
    fn rejects_non_f32_tables() {
        let error = compile_valid("f16", |_, header, _| {
            header["embeddings"]["dtype"] = json!("F16");
        })
        .expect_err("only F32 is supported");
        assert_eq!(
            error,
            CompileError::UnsupportedDtype {
                found: "F16".into()
            }
        );
    }

    #[test]
    fn reports_missing_and_malformed_tensor_fields() {
        for (field, value) in [
            ("shape", Some(json!([5, "2"]))),
            ("dtype", Some(json!(17))),
            ("shape", None),
            ("dtype", None),
        ] {
            let message = compile_valid("tensor-field", |_, header, _| {
                let fields = header["embeddings"].as_object_mut().unwrap();
                if let Some(value) = &value {
                    fields.insert(field.into(), value.clone());
                } else {
                    fields.remove(field);
                }
            })
            .unwrap_err()
            .to_string();
            assert!(message.contains("model.safetensors"), "{message}");
            assert!(message.contains(field), "{message}");
            let expected = value.map_or_else(|| "missing".into(), |v| v.to_string());
            assert!(message.contains(&expected), "{message}");
        }
    }

    #[test]
    fn reports_json_syntax_errors_in_the_header() {
        let (tokenizer, header, data) = valid();
        let dir = write_model("bad-header-json", tokenizer, header, &data);
        let mut file = 1_u64.to_le_bytes().to_vec();
        file.push(b'{');
        fs::write(dir.path().join("model.safetensors"), file).unwrap();
        let message = compile(dir.path()).unwrap_err().to_string();
        assert!(
            message.contains("model.safetensors: invalid JSON"),
            "{message}"
        );
        assert!(message.contains("EOF"), "{message}");
        assert!(message.contains("line 1 column 1"), "{message}");
    }

    #[test]
    fn rejects_malformed_raw_headers() {
        let (tokenizer, header, data) = valid();
        let tensor = &header["embeddings"];
        let dir = write_model("raw-header", tokenizer, header.clone(), &data);
        for (raw, expected) in [
            (
                format!(r#"{{"embeddings":{{}},"embeddings":{tensor}}}"#),
                "embeddings",
            ),
            (
                format!(r#"{{"__metadata__":4,"embeddings":{tensor}}}"#),
                "__metadata__",
            ),
            (
                format!(r#"{{"__metadata__":{{"source":4}},"embeddings":{tensor}}}"#),
                "__metadata__",
            ),
            (format!(" {header}"), "header"),
        ] {
            let mut file = (raw.len() as u64).to_le_bytes().to_vec();
            file.extend_from_slice(raw.as_bytes());
            file.extend_from_slice(&data);
            fs::write(dir.path().join("model.safetensors"), file).unwrap();
            let message = compile(dir.path()).unwrap_err().to_string();
            assert!(message.contains(expected), "{message}");
        }
    }

    #[test]
    fn rejects_non_matrix_shapes() {
        let error = compile_valid("shape", |_, header, _| {
            header["embeddings"]["shape"] = json!([2, 2, 1]);
        })
        .expect_err("the table must be a matrix");
        assert_eq!(
            error,
            CompileError::InvalidShape {
                found: json!([2, 2, 1])
            }
        );
    }

    #[test]
    fn rejects_overflowing_matrix_shapes() {
        for shape in [[usize::MAX, 2], [1, usize::MAX / 4 + 1]] {
            let error = compile_valid("shape-overflow", |_, header, _| {
                header["embeddings"]["shape"] = json!(shape);
            })
            .expect_err("the tensor size exceeds the address space");
            assert_eq!(
                error,
                CompileError::InvalidShape {
                    found: json!(shape)
                }
            );
        }
    }

    #[test]
    fn rejects_offsets_that_disagree_with_the_shape() {
        let error = compile_valid("offsets", |_, header, _| {
            header["embeddings"]["data_offsets"] = json!([0, 12]);
        })
        .expect_err("12 bytes cannot fill a 2x2 f32 table");
        assert_eq!(
            error,
            CompileError::SizeMismatch {
                expected: 16,
                found: 12
            }
        );
    }

    #[test]
    fn identifies_the_first_non_finite_weight() {
        for weight in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let message = compile_valid("non-finite", |_, _, data| {
                data[8..12].copy_from_slice(&weight.to_le_bytes());
                data[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
            })
            .unwrap_err()
            .to_string();
            assert_eq!(
                message,
                "model.safetensors: non-finite weight at row 1, column 0"
            );
        }
    }

    #[test]
    fn rejects_tensors_past_the_end_of_the_file() {
        let error = compile_valid("past-end", |_, _, data| data.truncate(8))
            .expect_err("the tensor is cut short");
        assert!(matches!(
            error,
            CompileError::InvalidOffsets { bytes: 8, .. }
        ));
    }

    #[test]
    fn identifies_invalid_vocabulary_ids() {
        for value in [json!(-1), json!(1.5), json!("one"), json!(4294967296_u64)] {
            let message = compile_valid("invalid-id", |tokenizer, _, _| {
                tokenizer["model"]["vocab"]["hi"] = value.clone();
            })
            .unwrap_err()
            .to_string();
            for expected in [
                "tokenizer.json",
                "hi",
                "0 to 4294967295",
                &value.to_string(),
            ] {
                assert!(message.contains(expected), "{message}");
            }
        }
    }

    #[test]
    fn rejects_vocab_ids_outside_the_table() {
        let error = compile_valid("id-range", |tokenizer, _, _| {
            tokenizer["model"]["vocab"] = json!({"[UNK]": 0, "hi": 5});
        })
        .expect_err("id 5 is outside a two-row table");
        assert_eq!(
            error,
            CompileError::IdOutOfRange {
                token: "hi".into(),
                id: 5,
                rows: 2
            }
        );
    }

    #[test]
    fn rejects_extra_tensors_in_either_key_order() {
        for name in ["aardvark", "zebra"] {
            let error = compile_valid("extra-tensor", |_, header, _| {
                header[name] = header["embeddings"].clone();
            })
            .expect_err("the table must be the only tensor")
            .to_string();
            assert!(
                error.contains(name) && error.contains("embeddings"),
                "{error}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_vocab_ids() {
        let error = compile_valid("duplicate-id", |tokenizer, _, _| {
            tokenizer["model"]["vocab"]["hi"] = json!(0);
        })
        .expect_err("two tokens share a row")
        .to_string();
        assert!(error.contains("hi") && error.contains("id 0"), "{error}");
    }

    #[test]
    fn rejects_unused_table_rows() {
        let error = compile_valid("unused-row", |tokenizer, _, _| {
            tokenizer["model"]["vocab"] = json!({"[UNK]": 0});
        })
        .expect_err("one table row has no token")
        .to_string();
        assert!(
            error.contains("1 token") && error.contains("2 rows"),
            "{error}"
        );
    }

    #[test]
    fn rejects_invalid_word_limits() {
        for value in [
            json!("seven"),
            json!(null),
            json!(-1),
            json!(4_294_967_296_u64),
        ] {
            let error = compile_valid("word-limit", |tokenizer, _, _| {
                tokenizer["model"]["max_input_chars_per_word"] = value.clone();
            })
            .expect_err("only 32-bit unsigned word limits can be embedded")
            .to_string();
            assert!(
                error.contains("max_input_chars_per_word") && error.contains(&value.to_string()),
                "{error}"
            );
        }
    }

    #[test]
    fn rejects_offsets_that_do_not_cover_the_payload_exactly() {
        for (offsets, file_bytes) in [
            (json!([0, 16, 16]), 16),
            (json!([0, "16"]), 16),
            (json!([4, 20]), 20),
            (json!([0, 16]), 20),
        ] {
            let supplied = offsets.to_string();
            let error = compile_valid("data-offsets", |_, header, data| {
                header["embeddings"]["data_offsets"] = offsets;
                data.resize(file_bytes, 0);
            })
            .expect_err("the single tensor must cover all data")
            .to_string();
            assert!(error.contains("data_offsets"), "{error}");
            assert!(error.contains(&supplied), "{error}");
        }
    }

    #[test]
    fn rejects_empty_embedding_dimensions() {
        let error = compile_valid("empty-dim", |_, header, data| {
            header["embeddings"]["shape"] = json!([2, 0]);
            header["embeddings"]["data_offsets"] = json!([0, 0]);
            data.clear();
        })
        .expect_err("the embedding dimension must be positive");
        assert_eq!(
            error,
            CompileError::InvalidShape {
                found: json!([2, 0])
            }
        );
    }

    #[test]
    fn errors_name_the_file_and_the_problem() {
        let message = CompileError::UnsupportedDtype {
            found: "F16".into(),
        }
        .to_string();
        assert!(message.contains("model.safetensors"), "{message}");
        assert!(message.contains("F16"), "{message}");
    }
}
