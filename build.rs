fn main() {
    generate_model_assets();
}

/// Pre-compiles `model/tokenizer.json` and `model/model.safetensors` into the compact,
/// allocation-free formats the firmware reads straight out of flash.
///
/// Writes to `OUT_DIR`:
/// - `vocab.bin`: `u32 count`, `count x u32` entry offsets (sorted by token bytes), then
///   entries of `u32 id, u8 len, bytes`. All little-endian. Lets the device binary-search
///   the vocabulary in place, with no JSON parsing and no heap use.
/// - `model_meta.rs`: constants describing the embedding table and the tokenizer.
fn generate_model_assets() {
    use std::path::PathBuf;
    use std::{env, fs};

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let tokenizer_path = manifest.join("model/tokenizer.json");
    let model_path = manifest.join("model/model.safetensors");
    println!("cargo:rerun-if-changed={}", tokenizer_path.display());
    println!("cargo:rerun-if-changed={}", model_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    // --- tokenizer ---------------------------------------------------------------
    let tok: serde_json::Value = serde_json::from_slice(
        &fs::read(&tokenizer_path).expect("cannot read model/tokenizer.json"),
    )
    .expect("model/tokenizer.json is not valid JSON");
    let model = &tok["model"];
    assert_eq!(
        model["type"], "WordPiece",
        "only WordPiece tokenizers are supported"
    );
    assert_eq!(
        model["continuing_subword_prefix"], "##",
        "the runtime tokenizer hardcodes the `##` continuation prefix"
    );
    let unk_token = model["unk_token"].as_str().expect("no unk_token");
    let max_chars = model["max_input_chars_per_word"].as_u64().unwrap_or(100);

    let mut vocab: Vec<(&str, u32)> = model["vocab"]
        .as_object()
        .expect("no vocab")
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_u64().expect("vocab id") as u32))
        .collect();
    vocab.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let unk_id = vocab
        .iter()
        .find(|(t, _)| *t == unk_token)
        .expect("unk_token missing from vocab")
        .1;

    let mut blob = Vec::new();
    blob.extend((vocab.len() as u32).to_le_bytes());
    let index_at = blob.len();
    blob.resize(index_at + 4 * vocab.len(), 0);
    for (i, (token, id)) in vocab.iter().enumerate() {
        let off = blob.len() as u32;
        blob[index_at + 4 * i..index_at + 4 * i + 4].copy_from_slice(&off.to_le_bytes());
        assert!(token.len() <= u8::MAX as usize, "token too long: {token}");
        blob.extend(id.to_le_bytes());
        blob.push(token.len() as u8);
        blob.extend(token.as_bytes());
    }
    fs::write(out.join("vocab.bin"), &blob).unwrap();

    // --- safetensors header --------------------------------------------------------
    let mut header_len = [0u8; 8];
    {
        use std::io::Read;
        fs::File::open(&model_path)
            .expect("cannot open model/model.safetensors")
            .read_exact(&mut header_len)
            .unwrap();
    }
    let header_len = u64::from_le_bytes(header_len) as usize;
    let file_len = fs::metadata(&model_path).unwrap().len() as usize;
    let raw = fs::read(&model_path).unwrap();
    let header: serde_json::Value =
        serde_json::from_slice(&raw[8..8 + header_len]).expect("bad safetensors header");
    let (_, t) = header
        .as_object()
        .unwrap()
        .iter()
        .find(|(k, _)| *k != "__metadata__")
        .expect("no tensor in safetensors file");
    assert_eq!(t["dtype"], "F32", "only F32 embedding tables are supported");
    let shape = t["shape"].as_array().unwrap();
    let (rows, dim) = (
        shape[0].as_u64().unwrap() as usize,
        shape[1].as_u64().unwrap() as usize,
    );
    let start = 8 + header_len + t["data_offsets"][0].as_u64().unwrap() as usize;
    let end = 8 + header_len + t["data_offsets"][1].as_u64().unwrap() as usize;
    assert_eq!(
        end - start,
        rows * dim * 4,
        "tensor size does not match its shape"
    );
    assert!(end <= file_len, "tensor extends past the end of the file");
    assert!(
        vocab.iter().all(|(_, id)| (*id as usize) < rows),
        "vocab has ids outside the embedding table"
    );

    fs::write(
        out.join("model_meta.rs"),
        format!(
            "pub const VOCAB_SIZE: usize = {rows};\n\
             pub const DIM: usize = {dim};\n\
             pub const TENSOR_START: usize = {start};\n\
             pub const TENSOR_END: usize = {end};\n\
             pub const UNK_ID: u32 = {unk_id};\n\
             pub const MAX_CHARS_PER_WORD: usize = {max_chars};\n"
        ),
    )
    .unwrap();
}
