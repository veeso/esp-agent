# static-models-bm

[![CI](https://github.com/veeso/static-models-bm-s31/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/static-models-bm-s31/actions/workflows/ci.yml)
[![TruffleHog](https://github.com/veeso/static-models-bm-s31/actions/workflows/trufflehog.yml/badge.svg)](https://github.com/veeso/static-models-bm-s31/actions/workflows/trufflehog.yml)
[![zizmor](https://github.com/veeso/static-models-bm-s31/actions/workflows/zizmor.yml/badge.svg)](https://github.com/veeso/static-models-bm-s31/actions/workflows/zizmor.yml)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org)

Bare-metal runner for the [potion-2M](https://huggingface.co/minishlab) static
embedding model on ESP32-S3 and ESP32-S31 (`no_std`, Rust).

The model (7.2 MiB, F32, 29,528 tokens x 64 dims) and the tokenizer vocabulary are embedded in the firmware image and read in place from flash. Only small per-request scratch data uses the heap.

## How it works

- `build.rs` compiles `model/tokenizer.json` into a sorted binary vocabulary and reads the shape of `model/model.safetensors`. It fails the build if the model is not F32 or the sizes don't match.
- `src/tokenizer.rs` is a `no_std` BERT WordPiece tokenizer: normalize (clean text, CJK spacing, accent stripping, lowercase), split on whitespace and punctuation, then greedy longest-match WordPiece.
- `src/embedder.rs` looks up the token rows, mean-pools them, and optionally L2-normalizes (`normalize`, default `true`).
- `src/bin/main.rs` embeds sample sentences and runs an interactive serial
  search console that prints timings and ranked matches.

The tokenizer and pooling match the [`statembed`](https://github.com/AstraBert/statembed) crate exactly on a set of tricky inputs (accents, CJK, emoji, control characters, truncation).

## Requirements

- An ESP32-S3 or ESP32-S31-Korvo-1 with 16 MiB of flash.
- The ESP Rust toolchain installed with [`espup`](https://github.com/esp-rs/espup),
  including `rust-src`. The repository pins version 1.97.0.0:

  ```sh
  espup install --toolchain-version 1.97.0.0
  ```

- `espflash` 4.6.0 or newer.
- [just](https://just.systems) for task execution.
- [dprint](https://dprint.dev) 0.56.1 and nightly rustfmt for formatting.
- [cargo-deny](https://embarkstudios.github.io/cargo-deny/) for dependency
  policy.
- [TruffleHog](https://github.com/trufflesecurity/trufflehog) for secret
  scanning.
- [git-cliff](https://git-cliff.org) for changelog generation.
- [zizmor](https://docs.zizmor.sh) for GitHub Actions auditing.
- The model files in `model/`: `model.safetensors` and `tokenizer.json`.
- On ESP32-S31-Korvo-1, use the UART USB-C connector for flashing and the
  serial monitor.

On macOS with Homebrew, install the packaged tools with:

```sh
brew install cargo-deny dprint git-cliff just shellcheck trufflehog zizmor
rustup toolchain install nightly --profile minimal --component rustfmt
```

## Run

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
<timing> us total (<timing> us to embed the query)
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
just run_s3
just run_s31
just monitor
just fmt
just fmt_check
just clippy "-- -D warnings"
just doc
just deny
just scan_secrets
just check
just setup_githooks
just changelog_preview 0.1.0
just changelog 0.1.0
```

A recipe without a chip suffix runs the `_s3` recipe and then the `_s31` one.

### Run the quality gate

`just check` is the local quality gate. It verifies formatting, runs Clippy
with warnings denied, builds documentation with warnings denied, and checks the
dependency policy, for both chips. The crate builds only for the device
targets, so there are no host tests.

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
- TruffleHog secret scanning.
- zizmor workflow-security auditing.

## Use it in your code

```rust
let embedder = static_models_bm::embedder();
let vector = embedder.embed("Qdrant is a vector search engine written in Rust.");
```

The embedder is cheap to create because nothing is copied. Texts are truncated at 512 tokens.
