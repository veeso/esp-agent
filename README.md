# static-models-bm

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
  including `rust-src`.
- `espflash` 4.6.0 or newer.
- The model files in `model/`: `model.safetensors` and `tokenizer.json`.
- On ESP32-S31-Korvo-1, use the UART USB-C connector for flashing and the
  serial monitor.

## Run

Select exactly one chip feature per build. ESP32-S3 is the default. ESP32-S31
uses `--no-default-features --features esp32s31`.

### ESP32-S3

```bash
source ~/export-esp.sh
cargo run --release
```

This builds, flashes, and opens the serial monitor. Press `Ctrl+R` in the
monitor to reset the board and print the output again, or `Ctrl+C` to exit.
To watch output without reflashing, run `espflash monitor`.

### ESP32-S31-Korvo-1

Build the firmware without flashing:

```bash
cargo build --locked --release \
  --target riscv32imafc-unknown-none-elf \
  --no-default-features --features esp32s31
```

To flash it and open the serial monitor:

```bash
cargo run --locked --release \
  --target riscv32imafc-unknown-none-elf \
  --no-default-features --features esp32s31
```

This builds, flashes with the 16 MiB flash configuration, and opens the serial
monitor on the board UART USB-C connector.

Both targets print the startup details, list eight indexed sentences, and wait
at the interactive prompt. For example:

```
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

## Use it in your code

```rust
let embedder = static_models_bm::embedder();
let vector = embedder.embed("Qdrant is a vector search engine written in Rust.");
```

The embedder is cheap to create because nothing is copied. Texts are truncated at 512 tokens.
