//! Produces the constant `RawModel` expression used by `model!` to construct a `Model`.

use std::path::Path;

use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::LitStr;

use crate::compile::compile;

/// Expands the argument of `model!` into a constant `RawModel` expression.
///
/// A relative path is resolved against `manifest_dir`. Every failure becomes a
/// `compile_error!` spanning the string literal.
pub(crate) fn expand(input: TokenStream, manifest_dir: &Path) -> TokenStream {
    try_expand(input, manifest_dir).unwrap_or_else(|error| error.to_compile_error())
}

fn try_expand(input: TokenStream, manifest_dir: &Path) -> syn::Result<TokenStream> {
    let literal: LitStr = syn::parse2(input)?;
    let model = compile(&manifest_dir.join(literal.value()))
        .map_err(|error| syn::Error::new(literal.span(), error))?;

    let utf8 = |path: &Path| {
        path.to_str().map(str::to_owned).ok_or_else(|| {
            syn::Error::new(
                literal.span(),
                format!("the path {path} is not valid UTF-8", path = path.display()),
            )
        })
    };
    let model_path = utf8(&model.model_path)?;
    let tokenizer_path = utf8(&model.tokenizer_path)?;
    let vocab = Literal::byte_string(&model.vocab_blob);
    let tensor_start = Literal::usize_suffixed(model.tensor_start);
    let tensor_end = Literal::usize_suffixed(model.tensor_end);
    let vocab_size = Literal::usize_suffixed(model.vocab_size);
    let dim = Literal::usize_suffixed(model.dim);
    let unk_id = Literal::u32_suffixed(model.unk_id);
    let max_chars_per_word = Literal::usize_suffixed(model.max_chars_per_word);

    Ok(quote! {
        {
            #[repr(C, align(4))]
            struct Aligned<T>(T)
            where
                T: ?::core::marker::Sized;
            static MODEL_FILE: &Aligned<[::core::primitive::u8]> = &Aligned(*::core::include_bytes!(#model_path));
            static VOCAB: &Aligned<[::core::primitive::u8]> = &Aligned(*#vocab);
            const _: &[::core::primitive::u8] = ::core::include_bytes!(#tokenizer_path);
            RawModel {
                file: &MODEL_FILE.0,
                tensor_start: #tensor_start,
                tensor_end: #tensor_end,
                vocab: &VOCAB.0,
                vocab_size: #vocab_size,
                dim: #dim,
                unk_id: #unk_id,
                max_chars_per_word: #max_chars_per_word,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::expand;
    use crate::compile::compile;
    use crate::test_util::{valid, write_model};

    fn valid_folder(name: &str) -> tempfile::TempDir {
        let (tokenizer, header, data) = valid();
        write_model(name, tokenizer, header, &data)
    }

    #[test]
    fn expand_resolves_paths_against_the_manifest_dir() {
        let dir = valid_folder("expand-relative");
        let dir = dir.path();
        let parent = dir.parent().expect("the folder has a parent");
        let name = dir
            .file_name()
            .expect("the folder has a name")
            .to_string_lossy()
            .into_owned();

        let output = expand(quote!(#name), parent).to_string();

        let model_path = dir.join("model.safetensors").display().to_string();
        let inclusion = quote!(::core::include_bytes!(#model_path)).to_string();
        assert!(output.contains(&inclusion), "{output}");
        assert!(output.contains("RawModel"), "{output}");
    }

    #[test]
    #[cfg(unix)]
    #[cfg_attr(target_vendor = "apple", ignore = "APFS requires UTF-8 filenames")]
    fn expand_reports_non_utf8_model_paths() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        let model = valid_folder("expand-non-utf8");
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join(OsStr::from_bytes(b"manifest-\xff"));
        std::fs::create_dir(&manifest).unwrap();
        symlink(model.path(), manifest.join("model")).unwrap();

        let output = expand(quote!("model"), &manifest).to_string();
        assert!(output.contains("compile_error"), "{output}");
        assert!(output.contains("model.safetensors"), "{output}");
        assert!(output.contains("not valid UTF-8"), "{output}");
    }

    #[test]
    fn expand_tracks_the_tokenizer_file() {
        let dir = valid_folder("expand-tracks");
        let dir = dir.path();
        let name = dir.display().to_string();

        let output = expand(quote!(#name), std::path::Path::new("/unused")).to_string();

        let tokenizer_path = dir.join("tokenizer.json").display().to_string();
        let inclusion = quote!(::core::include_bytes!(#tokenizer_path)).to_string();
        assert!(output.contains(&inclusion), "{output}");
    }

    #[test]
    fn expand_reports_compile_errors() {
        let dir = std::env::temp_dir().join(format!(
            "esp-local-agent-macros-expand-missing-{pid}",
            pid = std::process::id()
        ));
        let message = compile(&dir)
            .expect_err("the folder does not exist")
            .to_string();
        let name = dir.display().to_string();

        let output = expand(quote!(#name), std::path::Path::new("/unused")).to_string();

        assert!(output.contains("compile_error"), "{output}");
        assert!(output.contains(&quote!(#message).to_string()), "{output}");
    }

    #[test]
    fn expand_rejects_non_string_input() {
        let output = expand(quote!(42), std::path::Path::new("/unused")).to_string();

        assert!(output.contains("compile_error"), "{output}");
        assert!(output.contains("expected string literal"), "{output}");
    }
}
