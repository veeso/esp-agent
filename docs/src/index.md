# esp-agent

[![CI](https://github.com/veeso/esp-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/esp-agent/actions/workflows/ci.yml)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org)

`esp-agent` is an early-stage Rust `no_std` library for running a small agent
entirely on an ESP32-S3 or ESP32-S31. Your firmware embeds the model and keeps
its inputs and results on the chip.

## What it does today

- `Embedder` turns text into vectors on the chip.
- `Knowledge` stores each vector with its original text and an application payload.
- `Agent::prompt` finds the stored texts closest to a query. `Agent::turn` uses
  a handler you provide to run a command and can keep context for a follow-up.

## Quick start

Add the library to your firmware as a Git dependency.

```toml
[dependencies]
esp-agent = { git = "https://github.com/veeso/esp-agent" }
```

[Download a model](models.md#download-a-model) into `model/` in your firmware
crate. Embed it with `model!` and create an `Embedder` to turn text into a vector.

```rust
use esp_agent::{Embedder, Model, model};

static POTION: Model = model!("model");

let mut embedder = Embedder::new(POTION);
embedder.config_mut().max_length = 128;
let vector = embedder.embed("Qdrant is a vector search engine written in Rust.");
```

To search text, insert it with a payload into a `Knowledge` store and query
that store through an agent. See [Search](search.md) for ranking and buffer reuse.

```rust
use esp_agent::{Agent, Embedder, Knowledge, Model, model};

static POTION: Model = model!("model");

let mut knowledge = Knowledge::new(Embedder::new(POTION));
knowledge.insert("Paris is the capital of France.", 1);
knowledge.insert("Water boils at 100 degrees Celsius.", 2);
let agent = Agent::new(POTION, &knowledge).expect("the store is embedded with POTION");
let best = agent.prompt("Which city is France's capital?", 1);
```

To handle commands, enable conversation memory and pass your `TurnHandler`
implementation as `home`. The [Turns](turns.md) guide shows a complete handler.

```rust
let mut agent = Agent::builder(POTION, &knowledge)
    .context(2)
    .build()
    .expect("the store is embedded with POTION");
let outcome = agent.turn("Turn on the lamp", &mut home);
```

## Firmware requirements

- Supply a global allocator, for example with `esp-alloc`.
- Size the application partition for the model and the rest of the firmware.
  In practice, Potion 2M needs a board with 16 MiB of flash. See
  [Choose a model](models.md#choose-a-model) for model sizes.

The library leaves HAL selection, chip setup, and linker configuration to your
firmware.

## Build and test on a computer

With `rustup` and `just` installed, run these commands from the repository root.
The host recipes use standard Rust and `just test` downloads Potion 2M on first use.

```sh
rustup toolchain install 1.97 --profile minimal
just build_host
just test
```

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

`esp-agent` is licensed under the [Apache License 2.0](https://github.com/veeso/esp-agent/blob/main/LICENSE).
