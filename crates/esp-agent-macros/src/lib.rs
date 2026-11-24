//! Compile-time model embedding for `esp-agent`.

mod catalog;
mod commands;
mod compile;
mod expand;

use std::path::PathBuf;

use proc_macro::TokenStream;
use quote::quote;

/// Compiles the declarations of `targets!` into a `RawCatalog` expression.
#[proc_macro]
pub fn compile_targets(input: TokenStream) -> TokenStream {
    catalog::expand(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Derives the parser of the commands that an action enum accepts.
#[proc_macro_derive(TurnCommands, attributes(command, commands))]
pub fn derive_turn_commands(input: TokenStream) -> TokenStream {
    commands::expand(input.into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Compiles the fields of the embedding model for the library macro.
#[proc_macro]
pub fn compile_model(input: TokenStream) -> TokenStream {
    let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return quote!(compile_error!("model! must be invoked from a Cargo build")).into();
    };
    expand::expand(input.into(), &PathBuf::from(manifest_dir)).into()
}

#[cfg(test)]
pub(crate) mod test_util {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::TempDir;

    /// Writes a model folder into the temporary directory and returns it.
    pub(crate) fn write_model(name: &str, tokenizer: Value, header: Value, data: &[u8]) -> TempDir {
        let dir = tempfile::Builder::new()
            .prefix(name)
            .tempdir()
            .expect("the temporary folder can be created");
        fs::write(dir.path().join("tokenizer.json"), tokenizer.to_string())
            .expect("tokenizer.json can be written");
        let header = header.to_string().into_bytes();
        let mut file = u64::try_from(header.len())
            .expect("the header length fits in u64")
            .to_le_bytes()
            .to_vec();
        file.extend_from_slice(&header);
        file.extend_from_slice(data);
        fs::write(dir.path().join("model.safetensors"), file)
            .expect("model.safetensors can be written");
        dir
    }

    /// Returns a tokenizer, a safetensors header, and table data that compile.
    pub(crate) fn valid() -> (Value, Value, Vec<u8>) {
        let tokenizer = json!({
            "model": {
                "type": "WordPiece",
                "unk_token": "[UNK]",
                "continuing_subword_prefix": "##",
                "vocab": {"[UNK]": 0, "hi": 1},
            }
        });
        let header = json!({
            "embeddings": {"dtype": "F32", "shape": [2, 2], "data_offsets": [0, 16]}
        });
        (tokenizer, header, vec![0; 16])
    }
}
