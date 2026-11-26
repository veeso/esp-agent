# Changelog

All notable changes to this project are documented in this file.

## 0.2.0

Released on 2026-11-26

### Added

- build target catalogs from devices discovered at runtime (#39)

> Firmware that learns its devices after startup, from a scan or a coordinator, can now build a DynamicTargetCatalog and use it with TurnCommands::handler like a targets! catalog. The target! macro builds each entry from runtime IDs, names, and reply aliases, with positional or named arguments. insert_choice and insert_group keep the distinction between choosing one device and acting on a whole group, and remove and clear refresh the catalog between turns. Every insertion is checked with the rules targets! applies at build time and returns a CatalogError without changing the catalog. The handler now accepts any TargetLookup, which both catalogs implement.

- **commands:** switch command languages at run time (#38)

> Firmware can replace the phrases its command handler understands while a
> conversation is running. The agent keeps its remembered targets and any
> pending question across the switch.
>
> A language is either compiled into the firmware or received while it runs:
>
> - vocabulary! declares the phrases of one language for an action enum. A
>   previous phrase such as "spegnila" refers to the remembered target
>   without naming it.
> - RuntimeVocabulary builds a language from strings, such as those a
>   coordinator sends. Each phrase is checked with the rules of vocabulary!
>   within VocabularyLimits, and a bad or oversized phrase returns a
>   VocabularyError instead of aborting. It matches input like a compiled
>   vocabulary and matching allocates nothing.
>
> Build a switchable handler with CommandHandler::new, select a language with
> set_vocabulary, and restore the derived patterns with Vocabulary::derived.
> Both methods accept either kind of vocabulary. The TurnCommands derive also
> implements NamedActions, so run-time phrases name the variants directly.
>
> The pattern rules move to the new esp-agent-patterns crate, which the
> macros and the library share. It must be published before them.

## 0.1.0

Released on 2026-11-25

### Added

- support the ESP32-C5, ESP32-C6, and ESP32-P4 (#33)

> - feat: support the ESP32-C5, ESP32-C6, and ESP32-P4
>
> The example firmware now builds for every Espressif chip that can hold and
> map the 8.2 MB potion-2M image: ESP32-S3, ESP32-S31, ESP32-C5, ESP32-C6, and
> ESP32-P4. The RISC-V chips link with local copies of the esp-hal linker
> scripts whose ROM region is 16 MB, and build.rs picks them by chip feature
> because two chips share each RISC-V target.
>
> CI builds and lints every supported chip. The README and the new Supported
> Boards page list the supported chips and explain why the ESP32, ESP32-S2,
> ESP32-C2, ESP32-C3, ESP32-C61, and ESP32-H2 cannot hold the example model.
>
> - chore: point the PR template at the supported boards page

## 0.1.0-rc.1

Released on 2026-11-25

### Breaking changes

- turn the project into a library with an example firmware

> the crate no longer has the esp32s3 and esp32s31 features or a binary target; build the firmware from examples/semantic-search instead.

- load models with the model! macro (#10)

> VOCAB_SIZE, DIM, embedding_table, tokenizer, embedder, StaticEmbedder, EmbedderError, Vocab, and the console module are removed. Use model! and Embedder instead.

- rename the crate suite and repository to esp-agent (#13)

> the crate is now named esp-agent and is imported as
> esp_agent. Update the dependency name and the use paths in firmware.

- query stored texts through an agent (#23)

> Retriever is no longer public. Fill a Knowledge store and query it with Agent::prompt.

- refactor: use the agent in the example, benchmarks, and readme

* **agent:** add optional conversation context (#25)

> Agent owns its conversation memory, so it is no longer Copy and is Clone only when its payload is Clone.

### Added

- add interactive serial console for semantic search

> Replace the fixed demo in main with a prompt over the serial port. The
> firmware indexes a small set of sentences at boot, then embeds each typed
> line and prints the closest matches with cosine scores and timings.
>
> Input is read from both USB Serial/JTAG and UART0, since esp-println
> sends output to whichever one is active. Line editing lives in the new
> sans-IO console module.

- add ESP32-S31 firmware support

> Select the chip with Cargo features and link the full model on S31.

- add retrieval over stored texts (#14)

> Adds Index, an in-memory vector index with a payload per entry, and Retriever, which pairs it with an Embedder to search stored texts. Hits come back by descending cosine similarity, with ties in insertion order. The example firmware now searches with the retriever.

- Breaking: query stored texts through an agent (#23)

> - feat: add a shareable knowledge store
> - feat!: query stored texts through an agent

- Breaking: **agent:** add optional conversation context (#25)

> Resolve follow-up commands and demonstrate the flow on ESP32.

- declare device catalogs and turn commands

> Add typed command patterns, target lookup, and a firmware-owned brightness follow-up.

### Changed

- Breaking: turn the project into a library with an example firmware

> The firmware moves to the examples/semantic-search workspace crate, which owns the chip features, the heap, and the linker configuration. The root crate is now a chip-agnostic library that other ESP projects can depend on without inheriting linker settings.

- Breaking: load models with the model! macro (#10)

> Adds the esp-local-agent-macros crate. Its model! macro compiles any WordPiece model folder at build time into a Model value, so firmware can embed as many models as it needs. Embedder replaces StaticEmbedder and carries an EmbedderConfig that can be changed at run time. The potion-2M model and the serial console move into the example firmware.

- Breaking: rename the crate suite and repository to esp-agent (#13)

> Rename esp-local-agent to esp-agent and esp-local-agent-macros to
> esp-agent-macros, including the crate directories, the workspace
> manifests, the import path, the just recipes, the README, and the
> repository URLs.

### Performance

- add host benchmarks for tokenizing, embedding, and search (#21)

> Add the unpublished esp-agent-bench crate with criterion benchmarks in three groups: tokenize (normalization and WordPiece encoding), embed (embedding text and pooling), and search (storing texts and searching them with the Retriever and Index over 8 to 4096 texts). just bench_all runs every group and just bench <group> runs one. The Benchmark workflow runs them on every pull request, on every push to main, and on manual start, uploads target/criterion as the criterion-report artifact for 30 days, writes a table of the results with the new bench_report binary to the job summary, and posts it as a single updated comment on pull requests from this repository. The bench crate is excluded from the chip Clippy runs and linted on the host instead.

- reuse buffers when embedding and searching (#19)

> Adds Scratch and the encode_with, embed_with, search_into, and search_with methods. With warm buffers they allocate nothing, which a counting allocator in the tests checks. Normalization now runs in one pass, vocabulary lookups no longer build a key string, and search keeps only the top k hits. The existing methods keep their signatures and results.
