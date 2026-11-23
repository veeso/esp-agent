fn main() {
    linker_be_nice();
    generate_model_assets();
    configure_linker();
}

#[cfg(unix)]
fn check_xtensa_linker_available(target: &str) {
    println!("cargo:rerun-if-env-changed=PATH");

    println!(
        "cargo:rerun-if-env-changed={}",
        cargo_linker_env_var(target)
    );

    let linker = xtensa_linker(target);

    let error = match std::process::Command::new(&linker)
        .arg("--version")
        .output()
    {
        Ok(_) => return,
        Err(error) => error,
    };

    let export_file = std::env::var("HOME")
        .map(|home| format!("{home}/export-esp.sh"))
        .unwrap_or_else(|_| "$HOME/export-esp.sh".to_string());

    panic!(
        "Xtensa linker `{linker}` was not found in PATH or could not be executed: {error}.\n\n\
         Xtensa targets on Unix require espup's environment export file to be sourced before building.\n\
         Try: `source {export_file}`\n\n\
         If the export file does not exist, run `espup install` first.\n\
         For more details, see:\n\
         https://github.com/esp-rs/espup?tab=readme-ov-file#environment-variables-setup"
    );
}

#[cfg(not(unix))]
fn check_xtensa_linker_available(_: &str) {}

#[cfg(unix)]
fn xtensa_linker(target: &str) -> String {
    if let Some(linker) = cargo_configured_linker(target) {
        return linker;
    }

    target
        .strip_prefix("xtensa-")
        .and_then(|target| target.strip_suffix("-none-elf"))
        .map(|chip| format!("xtensa-{chip}-elf-gcc"))
        .unwrap_or_else(|| "xtensa-esp32-elf-gcc".to_string())
}

#[cfg(unix)]
fn cargo_configured_linker(target: &str) -> Option<String> {
    std::env::var(cargo_linker_env_var(target)).ok()
}

#[cfg(unix)]
fn cargo_linker_env_var(target: &str) -> String {
    let target = target.replace('-', "_").to_ascii_uppercase();
    format!("CARGO_TARGET_{target}_LINKER")
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }
}

fn configure_linker() {
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for build scripts");
    let error_handling_script = std::env::current_exe().unwrap();

    if target.starts_with("xtensa-") {
        check_xtensa_linker_available(&target);
        println!(
            "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
            error_handling_script.display()
        );
        println!("cargo:rustc-link-arg=-Tlinkall.x");
    } else if target == "riscv32imafc-unknown-none-elf" {
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
        );
        let linker_dir = manifest_dir.join("ld/esp32s31");
        let linker_script = linker_dir.join("linkall.x");

        println!("cargo:rerun-if-changed={}", linker_script.display());
        println!(
            "cargo:rerun-if-changed={}",
            linker_dir.join("memory-esp32s31.x").display()
        );
        println!("cargo:rustc-link-search={}", linker_dir.display());
        println!(
            "cargo:rustc-link-arg=--error-handling-script={}",
            error_handling_script.display()
        );
        println!("cargo:rustc-link-arg=-T{}", linker_script.display());
    }
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
    use std::{env, fs, path::PathBuf};

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
