//! Checks that firmware can embed several models from their own folders.

use esp_agent::{Embedder, Model, model};

static TINY_2D: Model = model!("tests/fixtures/tiny-2d");
static TINY_3D: Model = model!("tests/fixtures/tiny-3d");

#[test]
fn models_report_their_shape() {
    assert_eq!((TINY_2D.dim(), TINY_2D.vocab_size()), (2, 5));
    assert_eq!((TINY_3D.dim(), TINY_3D.vocab_size()), (3, 2));
}

#[test]
fn model_tokenizer_uses_its_own_unknown_id_and_word_limit() {
    let tokenizer = Embedder::new(TINY_2D).tokenizer();
    assert_eq!(tokenizer.encode_with_unk("hello hotdog"), [1, 4]);
    assert_eq!(tokenizer.encode("hello hotdog"), [1]);
}

#[test]
fn two_models_embed_side_by_side() {
    assert_eq!(Embedder::new(TINY_2D).embed("hello"), [1.0, 0.0]);
    assert_eq!(Embedder::new(TINY_3D).embed("hello"), [0.0, 0.0, 1.0]);
}

#[test]
fn macro_uses_builtin_names_when_the_caller_shadows_them() {
    #[expect(unused_imports, reason = "the imports exercise macro hygiene")]
    use core::{compile_error as include_bytes, marker::Copy as Sized, primitive::bool as u8};

    let model = model!("tests/fixtures/tiny-2d");
    assert_eq!(model.dim(), 2);
}
