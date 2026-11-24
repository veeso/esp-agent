# esp-agent

[![CI](https://github.com/veeso/esp-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/ci.yml)
[![TruffleHog](https://github.com/veeso/esp-agent/actions/workflows/trufflehog.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/trufflehog.yml)
[![zizmor](https://github.com/veeso/esp-agent/actions/workflows/zizmor.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/zizmor.yml)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org)

A bare-metal (`no_std`, Rust) local agent library for ESP32-S3 and ESP32-S31.

The goal of `esp-agent` is to run a small agent entirely on the chip, with
prompts, skills, and context embedded in the firmware image and nothing sent to
the cloud. It is a library that you add to your own ESP firmware project. The
project is at an early stage: today it ships the first building block, an
on-device text embedder, plus an example firmware with a semantic search
console.

Each firmware chooses which static embedding models to include. The semantic
search example embeds [potion-2M](https://huggingface.co/minishlab) and reads
its 7.2 MiB embedding table (F32, 29,528 tokens x 64 dims) and tokenizer
vocabulary in place from flash. Only small per-request scratch data uses the heap.

## Roadmap

- [x] On-device text embeddings and semantic search with potion-2M.
- [ ] Embeddable prompts compiled into the firmware image.
- [ ] Embeddable skills the agent can select and run.
- [ ] Embeddable context the agent can retrieve with the embedder.
- [ ] A local agent loop that ties prompts, skills, and context together.

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

Then store texts and search them by meaning:

```rust
use esp_agent::{Embedder, Model, Retriever, model};

static POTION: Model = model!("model");

let mut retriever = Retriever::new(Embedder::new(POTION));
retriever.insert("Paris is the capital of France.", 1);
retriever.insert("Water boils at 100 degrees Celsius.", 2);
let best = retriever.search("Which city is France's capital?", 1);
```

Each hit holds the score, the insertion index, and references to both the original
text and its payload.
Scores are cosine similarities, so they stay correct when the embedder's
`normalize` setting is off. Hits come back by descending score, and entries with
equal scores keep their insertion order. A `k` larger than the number of
entries returns every entry, and a `k` of zero returns none. A query with no
known words scores every entry `0.0`, so the hits are the first entries in
insertion order. The retriever gives read-only access to its embedder, because
changing the embedder settings after inserting texts would mix vectors built
with different settings.

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
- `crates/esp-agent/src/retriever.rs` is `Retriever`, which pairs an `Embedder` with an `Index` so callers store and search text.
- `examples/semantic-search` is an example firmware. It embeds the potion-2M
  model from `examples/semantic-search/model/`, indexes sample sentences with a
  `Retriever`, and runs an interactive serial search console that prints the
  search time and ranked matches. The sans-IO line editor for the console lives in its
  `src/console.rs`.

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
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/) for dependency
  policy.
- [TruffleHog](https://github.com/trufflesecurity/trufflehog) for secret
  scanning.
- [git-cliff](https://git-cliff.org) for changelog generation.
- [zizmor](https://docs.zizmor.sh) for GitHub Actions auditing.
- The model files in `examples/semantic-search/model/`: `model.safetensors` and
  `tokenizer.json`.
- On ESP32-S31-Korvo-1, use the UART USB-C connector for flashing and the
  serial monitor.

On macOS with Homebrew, install the packaged tools with:

```sh
brew install cargo-deny dprint git-cliff just shellcheck trufflehog zizmor
rustup toolchain install nightly --profile minimal --component rustfmt
```

## Run the example

The build and run recipes target the `semantic-search` example firmware.
Select exactly one chip per build. Every command goes through a `just` recipe:
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
just run_s3
just run_s31
just monitor
just lock
just fmt
just fmt_check
just clippy "-- -D warnings"
just doc
just test
just deny
just scan_secrets
just check
just setup_githooks
just changelog_preview 0.1.0
just changelog 0.1.0
```

For recipes with both chip variants, the unsuffixed recipe runs `_s3` and then `_s31`.
`build_host` and `test` run on your computer with the standard Rust toolchain.

### Run the quality gate

`just check` is the local quality gate. It checks formatting and runs Clippy and
rustdoc with warnings denied for each chip. It builds the library for your
computer with `just build_host`, runs the library tests once on it with
`just test`, and checks the workspace dependency policy once with `just deny`.

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
- A library build and the library tests on a regular computer (Linux), with
  the standard Rust toolchain.
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
