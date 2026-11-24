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
the agent asks the user to choose. With no remembered targets, it requests a
more specific command.

Each agent owns its conversation state. It can be cloned when its payload is
`Clone`, and each clone keeps an independent conversation. `Agent` never
implements `Copy`.

## Example with the home assistant

The `home-assistant` firmware controls two simulated lamps. Its knowledge
store holds one example for each supported action.

```rust
{{#include ../../examples/semantic-search/src/home.rs:examples}}
```

The firmware embeds the examples and gives the agent room for two targets.

```rust
{{#include ../../examples/semantic-search/src/home_assistant.rs:agent_setup}}
```

The handler reads brightness and target names from the user's input. It treats
“lamp” as ambiguous and “living room lights” as a group containing both lamps.

```rust
{{#include ../../examples/semantic-search/src/home.rs:handler}}
```

From the repository root, flash the example with the command for your board.

```sh
just run_s3 home-assistant
# ESP32-S31
just run_s31 home-assistant
```

Try these commands in the serial monitor.

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

The same walkthrough runs on your computer with `just test` in the
[home assistant integration test][home-assistant-test].

[home-assistant-test]: https://github.com/veeso/esp-agent/blob/main/crates/esp-agent/tests/home_assistant.rs
