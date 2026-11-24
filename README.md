# esp-agent

[![CI](https://github.com/veeso/esp-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/ci.yml)
[![Coverage](https://coveralls.io/repos/github/veeso/esp-agent/badge.svg?branch=main)](https://coveralls.io/github/veeso/esp-agent?branch=main)
[![TruffleHog](https://github.com/veeso/esp-agent/actions/workflows/trufflehog.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/trufflehog.yml)
[![zizmor](https://github.com/veeso/esp-agent/actions/workflows/zizmor.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/zizmor.yml)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org)
[![Docs](https://img.shields.io/badge/docs-veeso.github.io-blue)](https://veeso.github.io/esp-agent/)

A bare-metal (`no_std`, Rust) local agent library for ESP32-S3 and ESP32-S31.

The goal of `esp-agent` is to run a small agent entirely on the chip, with
prompts, skills, and context embedded in the firmware image and nothing sent to
the cloud. It is a library that you add to your own ESP firmware project. The
project is at an early stage: today it ships the first building block, an
on-device text embedder, plus an example firmware with a semantic search
console.

Each firmware chooses which [static embedding models](#embedding-models) to
include. The semantic search example embeds `potion-base-2M` and reads
its 7.2 MiB embedding table (F32, 29,528 tokens x 64 dims) and tokenizer
vocabulary in place from flash. A reusable `Scratch` retains heap capacity between
queries. Calls that fit those buffers need no new allocation for scratch data.

Read the [documentation website](https://veeso.github.io/esp-agent/) for guides
to choosing a model, searching stored texts, and handling conversation turns.

## Use it as a library

The crate is not published on crates.io yet. Add it to your firmware project as
a Git dependency:

```toml
[dependencies]
esp-agent = { git = "https://github.com/veeso/esp-agent" }
```

If your firmware uses a local checkout, append `crates/esp-agent` to
the dependency path that previously pointed to the repository root. For a
sibling checkout, use this entry in your firmware's `Cargo.toml`:

```toml
[dependencies]
esp-agent = { path = "../esp-agent/crates/esp-agent" }
```

Then embed text from your firmware:

```rust
use esp_agent::{Embedder, Model, model};

static POTION: Model = model!("model");

let mut embedder = Embedder::new(POTION);
embedder.config_mut().max_length = 128;
let vector = embedder.embed("Qdrant is a vector search engine written in Rust.");
```

Then store texts in a `Knowledge` store and ask an agent about them:

```rust
use esp_agent::{Agent, Embedder, Knowledge, Model, model};

static POTION: Model = model!("model");

let mut knowledge = Knowledge::new(Embedder::new(POTION));
knowledge.insert("Paris is the capital of France.", 1);
knowledge.insert("Water boils at 100 degrees Celsius.", 2);
let agent = Agent::new(POTION, &knowledge).expect("the store is embedded with POTION");
let best = agent.prompt("Which city is France's capital?", 1);
```

`Agent::new` fails with `AgentError::ModelMismatch` when the store was embedded
with another model. Any number of agents can borrow the same store.

Each hit returned by `prompt` holds the score, the insertion index, and references to both the original
text and its payload.
Scores are cosine similarities, so they stay correct when the embedder's
`normalize` setting is off. Hits come back by descending score, and entries with
equal scores keep their insertion order. A `k` larger than the number of
entries returns every entry, and a `k` of zero returns none. A prompt with no
known words scores every entry `0.0`, so the hits are the first entries in
insertion order. The store gives read-only access to its embedder, because
changing the embedder settings after inserting texts would mix vectors built
with different settings.

### Hold a conversation

An agent can also run commands and remember what a follow-up refers to. Enable
the memory with the builder, give `Agent::turn` a `TurnHandler` that knows your
devices, and the agent asks which device you mean, keeps the choice, and
resolves "it" in the next command:

```rust
let mut agent = Agent::builder(POTION, &knowledge)
    .context(2)
    .build()
    .expect("the store is embedded with POTION");
let outcome = agent.turn("Turn on the lamp", &mut home);
```

`context(2)` keeps at most two targets in each list: the choices of a pending
question and the targets of the last successful command. Without `context`, or
with zero, the agent keeps no memory and explicit commands still work. The
handler parses the original text, resolves targets in its own catalog, and
dispatches the action. A search hit is only a suggestion and never runs a
command. `turn` returns `Executed`, `Failed`, `Clarify`, `NeedTarget`, or
`Unrecognized`.

An agent owns its conversation, so it is no longer `Copy`, and it is `Clone`
only when its payload is. Each clone has an independent conversation.

A firmware loop can prompt without allocating by keeping one `Scratch` and one
hits vector, and reusing them for every prompt:

```rust
extern crate alloc;

use alloc::vec::Vec;
use esp_agent::Scratch;

let mut scratch = Scratch::new();
let mut hits = Vec::with_capacity(3);
agent.prompt_with("Which city is France's capital?", 3, &mut scratch, &mut hits);
```

`encode_with`, `embed_with`, `search_into`, and `prompt_with` allocate only when
a buffer needs to grow. Reserving room for `k` hits covers any search returning
up to `k` entries. Use `agent.knowledge().text(hit.index)` to borrow the original text
for a hit.

The library ships no model. `model!` embeds the folder you name, which holds
`tokenizer.json` and `model.safetensors`, and a relative path is resolved
against your firmware crate. The embedder is cheap to create because nothing is
copied, and `config_mut` changes the settings (the truncation length and the
normalization) while the firmware runs. A firmware can call `model!` several
times to embed several models and build one `Embedder` for each.

If your firmware uses `esp_agent::DEFAULT_MAX_LENGTH`, replace it with
`esp_agent::EmbedderConfig::DEFAULT_MAX_LENGTH`.

The library does not depend on a HAL and does not configure the linker, so your
project keeps full control of the chip setup. Your firmware must:

- provide a global allocator, for example with `esp-alloc`;
- have an application partition large enough for the models it embeds. The
  7.2 MiB potion-2M model needs, in practice, a chip with 16 MiB of flash.

See [`examples/semantic-search`](examples/semantic-search) for a complete
firmware that uses the library.

## Embedding models

Start with `potion-base-2M` for general English search on a 16 MiB board. If
your application partition has room, `potion-base-4M` scores higher on
Minish Lab's [retrieval benchmarks][potion-4m]. For biomedical English text,
try `pubmedbert-base-embeddings-2M`, which NeuML evaluated on
[PubMed datasets][pubmedbert-2m].

These models use BERT WordPiece and a single F32 embedding table. Flash sizes
include the weights and compiled vocabulary, but not the rest of the firmware.

| Model                           | Model data in flash | Parameters | Hugging Face            |
| ------------------------------- | ------------------: | ---------: | ----------------------- |
| `potion-base-2M`                |            7.64 MiB |  1,889,792 | [Minish Lab][potion-2m] |
| `pubmedbert-base-embeddings-2M` |            7.90 MiB |  1,953,408 | [NeuML][pubmedbert-2m]  |
| `potion-base-4M`                |           14.85 MiB |  3,779,584 | [Minish Lab][potion-4m] |

From your firmware crate directory, download `tokenizer.json` and
`model.safetensors` from the same model revision into `model/`. For example,
to use the same Potion 2M revision as this repository's tests:

```sh
base=https://huggingface.co/minishlab/potion-base-2M/resolve/389b9f64be5aa4ae7a6bc6fe95ef20ce485ae5da
mkdir -p model &&
  curl --fail --location "$base/tokenizer.json" -o model/tokenizer.json &&
  curl --fail --location "$base/model.safetensors" -o model/model.safetensors
```

Use `static POTION: Model = model!("model");` with `Embedder::new(POTION)` as
shown in [Use it as a library](#use-it-as-a-library), then rebuild your firmware.
Check that the resulting image fits the application partition before flashing,
especially with Potion 4M: 16 MiB of total flash does not guarantee it will fit.
Rebuild any stored embeddings when you switch models.

[potion-2m]: https://huggingface.co/minishlab/potion-base-2M
[pubmedbert-2m]: https://huggingface.co/NeuML/pubmedbert-base-embeddings-2M
[potion-4m]: https://huggingface.co/minishlab/potion-base-4M

## How it works

- `crates/esp-agent/src/lib.rs` defines the public `model!` macro. Its
  `compile_model!` helper in `crates/esp-agent-macros/` turns
  `tokenizer.json` into a sorted binary vocabulary and reads the shape of
  `model.safetensors` while the firmware builds. It fails the build if the
  table contains non-finite weights, has a dtype other than F32, or its size disagrees with its shape.
- `crates/esp-agent/src/model.rs` is the `Model` value that `model!` produces.
- `crates/esp-agent/src/tokenizer.rs` is a `no_std` BERT WordPiece tokenizer: normalize (clean text, CJK spacing, accent stripping, lowercase), split on whitespace and punctuation, then greedy longest-match WordPiece.
- `crates/esp-agent/src/embedder.rs` looks up the token rows, mean-pools them, and optionally L2-normalizes (`normalize`, default `true`). Its settings live in `EmbedderConfig`.
- `crates/esp-agent/src/index.rs` is `Index`, a brute-force in-memory vector index: one contiguous buffer of floats plus a payload per entry, searched by cosine similarity.
- `crates/esp-agent/src/knowledge.rs` is `Knowledge`, the store that pairs an `Embedder` with an `Index` and keeps the original texts.
- `crates/esp-agent/src/agent.rs` is `Agent`, which borrows a `Knowledge` store, answers prompts with the closest stored texts, and runs conversation turns. `crates/esp-agent/src/context.rs` holds its bounded conversation memory and the `TurnHandler` types. `crates/esp-agent/src/retriever.rs` is the internal search it forwards to.
- `examples/semantic-search` is an example firmware. It embeds the potion-2M
  model from `examples/semantic-search/model/` (downloaded, not tracked by
  git), indexes sample sentences in a
  `Knowledge` store, and runs an interactive serial search console that prints the
  search time and ranked matches. The sans-IO line editor for the console lives in its
  `src/console.rs`. A second binary, `home-assistant`, drives two simulated lamps
  through `Agent::turn`. Its catalog and vocabulary live in `src/home.rs`, and
  the serial and chip setup both binaries share lives in `src/board.rs`.

The tokenizer produces the same token IDs as [`statembed`](https://github.com/AstraBert/statembed) on inputs covering accents, CJK, emoji, control characters, and truncation.
Pooled embeddings agree within floating-point rounding precision.

## Build and test on a computer

The library also builds and runs on regular desktop and server computers with
the standard Rust toolchain. You do not need the ESP toolchain or a board for
this. CI checks it on Linux. The example firmware still needs a chip.

Install [rustup](https://rustup.rs), [just](https://just.systems), and the
Rust 1.97 toolchain:

```sh
rustup toolchain install 1.97 --profile minimal
```

Then build the library and run its tests for your computer:

```sh
just build_host
just test
```

Both recipes call `cargo +1.97 ... --target host-tuple`, which bypasses the ESP
toolchain pinned in `rust-toolchain.toml` and the ESP32-S3 default target in
`.cargo/config.toml`.

`just test` downloads the potion-2M model on first use, because the potion
tests embed it, and later runs reuse the downloaded files.

To measure test coverage, install
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) and the
`llvm-tools-preview` component, then run `just coverage`. It runs the same tests
and writes an LCOV report to `lcov.info`, the file CI uploads to
[Coveralls](https://coveralls.io/github/veeso/esp-agent):

```sh
rustup component add llvm-tools-preview --toolchain 1.97
just coverage
```

## Requirements

- An ESP32-S3 or ESP32-S31-Korvo-1 with 16 MiB of flash.
- The ESP Rust toolchain installed with [`espup`](https://github.com/esp-rs/espup),
  including `rust-src`. The repository pins version 1.97.0.0:

  ```sh
  espup install --toolchain-version 1.97.0.0
  ```

- The standard Rust 1.97 toolchain, for the library build and tests on your
  computer (see [Build and test on a computer](#build-and-test-on-a-computer)).

- `espflash` 4.6.0 or newer.
- [just](https://just.systems) for task execution.
- [dprint](https://dprint.dev) 0.56.1 and nightly rustfmt for formatting.
- [mdBook](https://rust-lang.github.io/mdBook/) 0.5.4 for the documentation website.
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/) for dependency
  policy.
- [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for test coverage.
- [TruffleHog](https://github.com/trufflesecurity/trufflehog) for secret
  scanning.
- [git-cliff](https://git-cliff.org) for changelog generation.
- [zizmor](https://docs.zizmor.sh) for GitHub Actions auditing.
- `curl` and `shasum`, which `just download_test_model` uses to download the
  potion-2M model into `examples/semantic-search/model/`. The build, run,
  Clippy, and test recipes run it for you, so you only run it by hand to
  prefetch the model.
- On ESP32-S31-Korvo-1, use the UART USB-C connector for flashing and the
  serial monitor.

On macOS with Homebrew, install the packaged tools with:

```sh
brew install cargo-deny cargo-llvm-cov dprint git-cliff just mdbook shellcheck trufflehog zizmor
rustup toolchain install nightly --profile minimal --component rustfmt
```

## Run the example

The build and run recipes target the example firmware package, which has two
binaries: `semantic-search` and `home-assistant`. Select exactly one chip per
build. Every command goes through a `just` recipe:
recipes with the suffix `_s3` or `_s31` run for one chip.

### ESP32-S3

```bash
source ~/export-esp.sh
just run_s3
```

This builds, flashes, and opens the serial monitor. Press `Ctrl+R` in the
monitor to reset the board and print the output again, or `Ctrl+C` to exit.
To watch output without reflashing, run `just monitor`. To build without
flashing, run `just build_s3`.

### ESP32-S31-Korvo-1

Build the firmware without flashing:

```bash
just build_s31
```

To flash it and open the serial monitor:

```bash
just run_s31
```

This builds, flashes with the 16 MiB flash configuration, and opens the serial
monitor on the board UART USB-C connector.

Both targets print the startup details, list eight indexed sentences, and wait
at the interactive prompt. For example:

```text
potion ready: 29528 tokens x 64 dims, table 7382 KiB in flash
indexed 8 sentences

Indexed sentences:
  - The quick brown fox jumps over the lazy dog.
  - A fast auburn fox leaps above a sleepy hound.
  - ...

> The quick brown fox jumps over the lazy dog.
<timing> us
  1. [<score>] The quick brown fox jumps over the lazy dog.
  2. [<score>] A fast auburn fox leaps above a sleepy hound.
  3. [<score>] ...

>
```

### Choose a demo

The run recipes take the demo as their first argument. Without it they flash
`semantic-search`:

```bash
just run_s3 home-assistant
just run_s31 home-assistant
just run_s3 semantic-search
```

An unknown name fails before anything is flashed. A second argument passes
Cargo flags, as in `just run_s3 home-assistant '--verbose'`. Cargo
flags alone, such as `just run_s3 '--verbose'`, still work and run
`semantic-search`.

### Talk to the simulated lamps

`home-assistant` controls two simulated lamps, a desk lamp and a floor lamp.
Both are called "lamp", and "living room lights" means both. It changes only
simulated state and drives no hardware. The agent keeps up to two targets. Type
this at the prompt:

```text
> Turn on the lamp
Which lamp, the desk lamp or the floor lamp?
> The desk one
The desk lamp is on.
> Dim it to 30 percent
The desk lamp is at 30 percent.
> Turn on the living room lights
The living room lights are on.
> Dim it to 30 percent
Which lamp, the desk lamp or the floor lamp?
> The floor one
The floor lamp is at 30 percent.
```

"It" means the last lamp only when that command acted on a single lamp. After
the command for both lamps the agent asks for a specific one.

## Troubleshooting

- **Permission denied on `/dev/ttyACM0`:** run `sudo usermod -aG dialout $USER`, then log out and in.
- **Board not listed or `Connecting...` fails:** use a data-capable USB cable. Hold BOOT, tap RESET, release BOOT, then try again.
- **`waiting for download` after flashing:** release BOOT and tap RESET, or unplug and replug the board.
- **Flash size errors:** add `--flash-size 16mb` to the runner in `.cargo/config.toml`.

## Development

### Common tasks

Run `just` to list every recipe.

```sh
just build
just build_s3
just build_s31
just build_host
just download_test_model
just run_s3
just run_s31
just monitor
just lock
just fmt
just fmt_check
just clippy "-- -D warnings"
just doc
just book
just book_serve
just test
just coverage
just test_bench
just bench_all
just bench tokenize
just bench_smoke
just bench_report
just clippy_bench "-- -D warnings"
just deny
just scan_secrets
just check
just setup_githooks
just changelog_preview 0.1.0
just changelog 0.1.0
```

For recipes with both chip variants, the unsuffixed recipe runs `_s3` and then `_s31`.
`build_host`, `test`, `coverage`, `test_bench`, `clippy_bench`, and the `bench` recipes run on
your computer with the standard Rust toolchain.

### Build the documentation website

From the repository root, build the site with mdBook 0.5.4.

```sh
just book
```

The entry page is `docs/book/index.html`. To preview the site and rebuild on
changes, run `just book_serve`.

### Run the quality gate

`just check` is the local quality gate. It checks formatting and runs Clippy and
rustdoc with warnings denied for each chip. It builds the library for your
computer with `just build_host`, lints the benchmarks with `just clippy_bench`,
runs the library tests and the benchmark report tests once on it with `just test`
and `just test_bench`, and checks the workspace dependency policy once with
`just deny`.

### Run the benchmarks

The `esp-agent-bench` crate holds criterion benchmarks that run on your computer
with the standard Rust toolchain. They use the potion-2M model of the example
firmware and cover three groups:

- `tokenize`: text normalization and WordPiece encoding.
- `embed`: turning text into vectors, and mean-pooling token ids.
- `search`: storing texts and storing texts in a `Knowledge` store, prompting an `Agent`, and searching an `Index`.

The benchmark recipes download the potion-2M model on first use, like `just test`.
The numbers come from a computer, so they do not match the chip. Use them to
tell whether a change makes the library faster or slower.

```sh
just bench_all
just bench tokenize
just bench_smoke
just bench_report
```

`just bench_all` runs every group, and `just bench <group>` runs one of
`tokenize`, `embed`, or `search`. `just bench_smoke` runs every benchmark once
without measuring, to check that they work. `just bench_report` prints the
results of the last run as a Markdown table. The HTML report is in
`target/criterion/report/index.html`.

### Enable the Git hooks

Install the tracked pre-commit hook with:

```sh
just setup_githooks
```

The hook scans staged files for secrets, checks formatting, runs Clippy, and
checks dependencies with cargo-deny.

### Update the changelog

Commits follow the
[Conventional Commits](https://www.conventionalcommits.org) specification.
Preview or generate release notes with:

```sh
just changelog_preview 0.1.0
just changelog 0.1.0
```

### Continuous integration

GitHub Actions provides:

- A build and Clippy check for each chip.
- Formatting, documentation, and dependency-policy checks.
- A library build, the library tests, a lint of the benchmarks, and the
  benchmark report tests on a regular computer (Linux), with the standard Rust
  toolchain.
- Benchmarks on a regular computer for every pull request and every push to
  `main`, also started by hand. The criterion report is uploaded as the
  `criterion-report` artifact for 30 days, and the results are written to the job
  summary. On pull requests from this repository the workflow also posts the
  numbers as one comment that each push updates.
- The documentation website builds on pull requests that touch the book or its
  examples and deploys to GitHub Pages from `main`.
- TruffleHog secret scanning.
- zizmor workflow-security auditing.

## Credits

`esp-agent` started as a fork of
[`static-models-bm`](https://github.com/qdrant-labs/static-models-bm) by
[qdrant-labs](https://github.com/qdrant-labs), created by
[AstraBert](https://github.com/AstraBert). The tokenizer, the embedder, and the
idea of running static embedding models on bare metal all come from that work.
Huge thanks and the utmost respect to AstraBert for it.

AstraBert is also the author of
[`statembed`](https://github.com/AstraBert/statembed), the reference that the
tokenizer and pooling are validated against.

## License

`esp-agent` is licensed under the [Apache License 2.0](LICENSE).
