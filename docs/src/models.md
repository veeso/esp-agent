# Available Models

`esp-agent` ships no model. Your firmware chooses which static embedding models
it needs and embeds them with `model!`.

## Choose a model

These models use BERT WordPiece and a single F32 embedding table. The sizes
cover the weights and compiled vocabulary. Budget separately for the rest of
your firmware.

| Model                           | Model data in flash | Parameters | Hugging Face            |
| ------------------------------- | ------------------: | ---------: | ----------------------- |
| `potion-base-2M`                |            7.64 MiB |  1,889,792 | [Minish Lab][potion-2m] |
| `pubmedbert-base-embeddings-2M` |            7.90 MiB |  1,953,408 | [NeuML][pubmedbert-2m]  |
| `potion-base-4M`                |           14.85 MiB |  3,779,584 | [Minish Lab][potion-4m] |

Start with `potion-base-2M` for general English search on a 16 MiB board. If
your application partition has room, `potion-base-4M` scores higher on
Minish Lab's [retrieval benchmarks][potion-4m]. For biomedical English text,
try `pubmedbert-base-embeddings-2M`, which NeuML evaluated on
[PubMed datasets][pubmedbert-2m].

## Download a model

Download `tokenizer.json` and `model.safetensors` from the same revision.
Run this from your firmware crate to fetch the Potion 2M revision used by the
examples and tests.

```sh
base=https://huggingface.co/minishlab/potion-base-2M/resolve/389b9f64be5aa4ae7a6bc6fe95ef20ce485ae5da
mkdir -p model &&
  curl --fail --location "$base/tokenizer.json" -o model/tokenizer.json &&
  curl --fail --location "$base/model.safetensors" -o model/model.safetensors
```

## Embed it

With `Model` and `model` imported from `esp_agent`, embed the directory and
rebuild your firmware. The path is relative to your firmware crate.

```rust
static POTION: Model = model!("model");
```

Use `Embedder::new(POTION)` to embed text. You can call `model!` multiple times
with different directories and create an `Embedder` for each model.

## What the build checks

`model!` compiles `tokenizer.json` into a sorted binary vocabulary and reads the
embedding table in `model.safetensors`. The build fails if the table uses a data
type other than F32, contains non-finite weights, or its size disagrees with
its shape.

## Switching models

Check that the rebuilt firmware fits the application partition before flashing.
Potion 4M alone occupies 14.85 MiB, so check the partition size even on a board
with 16 MiB of flash. Rebuild the embeddings in your store when you switch
models. `Agent::new` returns `AgentError::ModelMismatch` when the store was
embedded with another model.

[potion-2m]: https://huggingface.co/minishlab/potion-base-2M
[pubmedbert-2m]: https://huggingface.co/NeuML/pubmedbert-base-embeddings-2M
[potion-4m]: https://huggingface.co/minishlab/potion-base-4M
