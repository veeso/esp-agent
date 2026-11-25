# Supported Boards

`esp-agent` does not depend on a HAL, so the library itself compiles for every
Espressif target. What decides whether a chip can run an agent is flash: the
firmware reads the embedding model in place from flash, so the chip must both
have enough flash and map enough of it into its address space at once.

The example firmware embeds `potion-base-2M`. Its application image is about
8.2 MB, so it needs a board with 16 MB of flash. CI builds and lints the
example firmware for every supported chip on each pull request.

## Supported chips

| Chip      | Architecture | Rust target                     | Example feature | Recipes                |
| --------- | ------------ | ------------------------------- | --------------- | ---------------------- |
| ESP32-S3  | Xtensa       | `xtensa-esp32s3-none-elf`       | `esp32s3`       | `build_s3`, `run_s3`   |
| ESP32-S31 | RISC-V       | `riscv32imafc-unknown-none-elf` | `esp32s31`      | `build_s31`, `run_s31` |
| ESP32-C5  | RISC-V       | `riscv32imac-unknown-none-elf`  | `esp32c5`       | `build_c5`, `run_c5`   |
| ESP32-C6  | RISC-V       | `riscv32imac-unknown-none-elf`  | `esp32c6`       | `build_c6`, `run_c6`   |
| ESP32-P4  | RISC-V       | `riscv32imafc-unknown-none-elf` | `esp32p4`       | `build_p4`, `run_p4`   |

Pick a module or development board with 16 MB of flash, for example
ESP32-S3-WROOM-1-N16R8, ESP32-C5-WROOM-1-N16R4, ESP32-C6-WROOM-1-N16, the
ESP32-P4-Function-EV-Board, or the ESP32-S31-Korvo-1. `esp-hal` supports the
ESP32-P4 from chip revision v3.0 and the ESP32-S31 is still at an early stage
there.

The ESP32-C5 and ESP32-C6 have no floating-point unit, so they compute
embeddings with software floats and run slower than the other chips.

Build and flash the example for one of them:

```sh
just build_c6
just run_c6
just run_c6 home-assistant
```

## Unsupported chips

These chips cannot hold the example model, because their flash or their flash
mapping is smaller than the model:

| Chip      | Limit                                                          |
| --------- | -------------------------------------------------------------- |
| ESP32     | Maps at most 4 MB of flash for read-only data.                 |
| ESP32-S2  | Maps at most 4 MB of flash for read-only data.                 |
| ESP32-C2  | Maps at most 4 MB of flash for read-only data.                 |
| ESP32-C3  | Maps at most 8 MB of flash, less than the 8.2 MB image.        |
| ESP32-C61 | Modules ship with 8 MB of flash at most.                       |
| ESP32-H2  | Ships with 4 MB of in-package flash and takes no external one. |

The library still builds for them, so a firmware with a much smaller model,
under the limit in the table, can use it.

## Port your own firmware

The library leaves the chip setup to your firmware. To run on a supported chip:

1. Enable the chip feature of `esp-hal`, `esp-alloc`, `esp-backtrace`,
   `esp-bootloader-esp-idf`, and `esp-println`.
2. Build for the Rust target in the table.
3. On the RISC-V chips, link with a copy of the `esp-hal` linker scripts whose
   `ROM` region is 16 MB instead of 4 MB, as the example does in
   `examples/semantic-search/ld/`. With the stock scripts the link fails
   because the model does not fit the region.
4. Flash with a partition table whose application partition fits the image.
