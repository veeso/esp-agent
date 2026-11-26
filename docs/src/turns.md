# Turns

A turn handles one line of conversation and can run a command. Call
`Agent::turn` with the line and a `TurnHandler` supplied by your firmware.

## What it is good for

Use turns to control devices from serial text or from text produced by a
speech recognizer. The agent can ask which device the user means and let them
say “it” in a follow-up. The command and its context are handled on the chip.

## Who decides what

The agent handles the order of events in a turn and keeps conversation memory.
Your handler decides what the input means and how to act on it. When the
closest stored example has positive similarity, the agent passes its payload
as `suggested`. A search hit provides a hint only. `parse` validates the
original input, including every argument, before returning a command.

## The handler

- `parse` extracts a `Command` with an action and a `TargetSelector`.
  `Explicit` carries a device name, alias, or group. `Previous` refers to
  the target of the last successful command, as in “dim it”.
- `resolve_targets` looks up an explicit selector in your device catalog.
  Each device has a stable `TargetId`. `TargetMatch::Resolved` names the
  devices that act together, `Ambiguous` offers alternatives for the user to
  choose from, and `Missing` indicates that the selector matched no device.
- `target_available` is an optional check before offering members of a
  remembered group or matching a clarification reply. The default keeps every
  choice. If your catalog changes, return `false` for missing devices. When no
  choices remain, the agent returns `NeedTarget` without keeping a pending
  question.
- `matches_target` checks whether a clarification reply identifies a device
  in the choices offered. Exactly one match is needed to continue.
- `dispatch` runs the action once on the resolved targets. It checks that
  every target ID still belongs to the catalog and returns a result or error.

## Outcomes

`Agent::turn` returns a `TurnOutcome` that your firmware can use to respond.

| Outcome        | Meaning                                                           |
| -------------- | ----------------------------------------------------------------- |
| `Executed`     | The handler ran the action successfully and returned its result.  |
| `Failed`       | The handler returned an error while running the action.           |
| `Clarify`      | Ask the user to choose one of the returned targets.               |
| `NeedTarget`   | Ask for a complete command with a more specific target.           |
| `Unrecognized` | The input did not parse as a command, and no question is pending. |

## Conversation memory

Call `Agent::builder(model, &knowledge).context(n).build()` to keep up to `n`
choices for a pending question and up to `n` targets from the last successful
command. The default is zero, including when you use `Agent::new`. With memory
set to zero, explicit commands still work.

A successful dispatch replaces the remembered targets or clears them if the
whole list exceeds `n`. A failed dispatch preserves the previous targets.
“It” resolves directly when one target is remembered. If a group is remembered,
the agent asks the user to choose from members that are still available. If no
members remain or no targets are remembered, it requests a new command with a
more specific target (`NeedTarget`).

Each agent owns its conversation state. It can be cloned when its payload is
`Clone`, and each clone keeps an independent conversation. `Agent` never
implements `Copy`.

## Declare the vocabulary instead

You do not need to write the handler by hand. `targets!` declares the devices
and the phrases that name them, and `#[derive(TurnCommands)]` declares the
commands on your action enum. The [turn macros reference](reference/turn-macros.md)
covers every rule. The home assistant firmware declares both. To switch the
command language at run time, see [Switching languages](languages.md). The
[discovered devices example](#example-with-discovered-devices) builds the catalog
at run time and switches language during a conversation.

## Example with the home assistant

The `home-assistant` firmware controls two simulated lamps. Its knowledge
store holds examples for turning a lamp on and setting its brightness. For
“Dim the lamp”, the firmware constructs `BeginDim` directly.

```rust
{{#include ../../examples/semantic-search/src/home.rs:examples}}
```

The firmware embeds the examples and gives the agent room for two targets.

```rust
{{#include ../../examples/semantic-search/src/home_assistant.rs:agent_setup}}
```

The catalog lists the lamps. “Lamp” is ambiguous and “living room lights” is a
group containing both lamps.

```rust
{{#include ../../examples/semantic-search/src/home.rs:catalog}}
```

The handler forwards parsing and lookups to the declarations. It also keeps
one firmware-owned question, described below.

```rust
{{#include ../../examples/semantic-search/src/home.rs:handler}}
```

From the repository root, flash the example with the command for your board.

```sh
just run_s3 home-assistant
# ESP32-S31, ESP32-C5, ESP32-C6, or ESP32-P4
just run_s31 home-assistant
just run_c5 home-assistant
just run_c6 home-assistant
just run_p4 home-assistant
```

Try these commands in the serial monitor. The agent asks which lamp, and “it”
refers to the last lamp that a command acted on.

```text
> Dim the lamp to 37%
Which lamp, the desk lamp or the floor lamp?
> The desk one
The desk lamp is at 37 percent.
> Turn it on
The desk lamp is on.
> Turn on the living room lights
The living room lights are on.
> Dim it to 12%
Which lamp, the desk lamp or the floor lamp?
> Floor
The floor lamp is at 12 percent.
```

## A question that the firmware owns

The agent asks which target a command means. It cannot know that a command is
missing a value, so your firmware asks for that. “Dim the lamp” is a short
command that the firmware turns into the unannotated `BeginDim` action. After
the agent resolves the lamp, dispatching `BeginDim` records the lamp and returns
a prompt. It changes no hardware.

```rust
{{#include ../../examples/semantic-search/src/home.rs:brightness_followup}}
```

The conversation looks like this. An invalid value repeats the question.

```text
> Dim the lamp
Which lamp, the desk lamp or the floor lamp?
> The desk one
To what brightness?
> 150 percent
To what brightness?
> 37 percent
The desk lamp is at 37 percent.
```

The firmware keeps this state next to its devices, one value for each
conversation, and the question is independent of the agent's memory limit. A
new command abandons it, even a command that needs a clarification, names an
unknown lamp, or is malformed such as “Dim the floor lamp to 150%”, so a later “37 percent” cannot revive the old question.
If the serial line is too long, the firmware asks for a shorter command. A
command verb in the retained prefix still cancels the brightness question.
Classifying a new command and clearing the question happens in the firmware's
`respond` before it calls `Agent::turn`, because `parse` borrows the handler
immutably.

```text
> Dim the lamp to 37%
Which lamp, the desk lamp or the floor lamp?
> Dim desk lamp
To what brightness?
> Turn on the floor lamp
The floor lamp is on.
> 37 percent
Sorry, I did not understand. Try "Turn on the lamp".
```

A group has no single brightness question, so “Dim the living room lights”
replies that you must name one lamp. A complete command still works on a group.

The same walkthrough runs on your computer with `just test` in the
[home assistant integration test][home-assistant-test].

[home-assistant-test]: https://github.com/veeso/esp-agent/blob/main/crates/esp-agent/tests/home_assistant.rs

## Example with discovered devices

Firmware often learns its devices from a coordinator, such as a hub or a home
server, and lets the user pick the language of the commands. This example
follows one lamp, “Luna” with ID 42, through discovery, an English command, a
switch to Italian, and an Italian follow-up. It uses simulated coordinator data,
so it runs on your computer. From the repository root, run:

```sh
just test '--test discovered_devices'
```

The Rust blocks below are excerpts from the
[discovered devices test][discovered-test].

### Fill the catalog from discovery

The coordinator reports each device with a stable ID and a name. `target!`
turns one report into a catalog entry, and the name is what the user says in
every language.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:discovery}}
```

### Configure the command phrases

The action enum declares the English phrases:

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:actions}}
```

The other languages are a map of run-time vocabularies. Add a language or
change a phrase here. [Switching languages](languages.md) explains the rules.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:languages}}
```

A parser written by hand can accept phrases that no pattern spells. This one
adds “lights out” to English and returns `TargetSelector::Previous`, so the
agent turns off the lamp of the last command. Wrap it with `Vocabulary::new`.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:handwritten}}
```

### Act and reply

The dispatch callback receives the IDs the agent resolved. It is where firmware
sends the command to the real device.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:dispatch}}
```

The library returns a `TurnOutcome` and never writes text. Your firmware turns
the outcome into a reply, so edit `reply` to change or translate what the user
reads. It looks device names up in the catalog by ID.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:replies}}
```

### The conversation

The firmware starts in English and switches to Italian between turns. The agent
remembers Luna across the switch, so “Spegnila” acts on device 42, and each
assertion checks the ID that the callback received.

```rust
{{#include ../../crates/esp-agent/tests/discovered_devices.rs:walkthrough}}
```

```text
Coordinator supplies device 42 named Luna.
> Turn on Luna
Luna is on.
The device language changes to Italian.
> Spegnila
Luna è spenta.
```

[discovered-test]: https://github.com/veeso/esp-agent/blob/main/crates/esp-agent/tests/discovered_devices.rs
