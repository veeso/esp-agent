# Example firmware crate that the build and run recipes target.
example := "semantic-search"

# Cargo flag that selects the target of the ESP32-C5 and the ESP32-C6.
c5_target := "--target riscv32imac-unknown-none-elf"
c6_target := c5_target

# Cargo flag that selects the target of the ESP32-P4 and the ESP32-S31.
s31_target := "--target riscv32imafc-unknown-none-elf"
p4_target := s31_target

# Cargo flags that select each RISC-V chip: its target and the example chip feature.
c5_flags := c5_target + " --no-default-features --features " + example + "/esp32c5"
c6_flags := c6_target + " --no-default-features --features " + example + "/esp32c6"
p4_flags := p4_target + " --no-default-features --features " + example + "/esp32p4"
s31_flags := s31_target + " --no-default-features --features " + example + "/esp32s31"

# Cargo flag that sets the `espflash` runner of a chip whose target has no runner of its own in .cargo/config.toml.
c5_runner := "--config 'target.riscv32imac-unknown-none-elf.runner=\"espflash flash --monitor --chip esp32c5\"'"
c6_runner := "--config 'target.riscv32imac-unknown-none-elf.runner=\"espflash flash --monitor --chip esp32c6\"'"
p4_runner := "--config 'target.riscv32imafc-unknown-none-elf.runner=\"espflash flash --monitor --chip esp32p4\"'"

# Standard Rust toolchain for regular computers; keep it equal to `workspace.package.rust-version`.
host_toolchain := "1.97"

# Cargo toolchain override for regular computers, which bypasses the ESP toolchain in rust-toolchain.toml.
host_cargo := "cargo +" + host_toolchain

# Library packages that build and test on regular computers.
host_packages := "--package esp-agent --package esp-agent-macros"

# Folder the example firmware and the tests embed the model from.
example_model_dir := "examples/" + example + "/model"

# Pinned Hugging Face revision of the potion-2M model the example and tests use.
example_model_url := "https://huggingface.co/minishlab/potion-base-2M/resolve/389b9f64be5aa4ae7a6bc6fe95ef20ce485ae5da"

# Download the potion-2M model the example firmware and the tests embed.
[group('build')]
download_test_model:
  #!/usr/bin/env bash
  set -euo pipefail
  mkdir -p "{{ example_model_dir }}"
  for entry in \
    "model.safetensors f95ffde02ad06f63ae38eb9d400038cd5ccaf8411ec3cb650c6025113f96cbb8" \
    "tokenizer.json e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50"
  do
    read -r name checksum <<< "$entry"
    path="{{ example_model_dir }}/$name"
    if [ -f "$path" ] && echo "$checksum  $path" | shasum -a 256 -c --status; then
      continue
    fi
    echo "downloading $name"
    curl --fail --location --silent --show-error --output "$path.part" "{{ example_model_url }}/$name"
    if ! echo "$checksum  $path.part" | shasum -a 256 -c --status; then
      rm -f "$path.part"
      echo "checksum mismatch for $name from {{ example_model_url }}" >&2
      exit 1
    fi
    mv "$path.part" "$path"
  done

# Build the library for this computer with the standard Rust toolchain.
[group('build')]
build_host args="":
  {{ host_cargo }} build --locked {{ host_packages }} --target host-tuple {{ args }}

# Build the example firmware for the ESP32-S3 in release mode.
[group('build')]
build_s3 args="": download_test_model
  cargo build --locked --release --package {{ example }} {{ args }}

# Build the example firmware for the ESP32-S31 in release mode.
[group('build')]
build_s31 args="": download_test_model
  cargo build --locked --release --package {{ example }} {{ s31_flags }} {{ args }}

# Build the example firmware for the ESP32-C5 in release mode.
[group('build')]
build_c5 args="": download_test_model
  cargo build --locked --release --package {{ example }} {{ c5_flags }} {{ args }}

# Build the example firmware for the ESP32-C6 in release mode.
[group('build')]
build_c6 args="": download_test_model
  cargo build --locked --release --package {{ example }} {{ c6_flags }} {{ args }}

# Build the example firmware for the ESP32-P4 in release mode.
[group('build')]
build_p4 args="": download_test_model
  cargo build --locked --release --package {{ example }} {{ p4_flags }} {{ args }}

# Build the example firmware for every supported chip in release mode.
[group('build')]
build args="": (build_s3 args) (build_s31 args) (build_c5 args) (build_c6 args) (build_p4 args)

# Flash semantic-search (default) or home-assistant to the ESP32-S3 and open the serial monitor. Cargo flags are optional.
[group('build')]
run_s3 scenario="" args="": download_test_model (_run "" scenario args)

# Flash semantic-search (default) or home-assistant to the ESP32-S31 and open the serial monitor. Cargo flags are optional.
[group('build')]
run_s31 scenario="" args="": download_test_model (_run s31_flags scenario args)

# Flash semantic-search (default) or home-assistant to the ESP32-C5 and open the serial monitor. Cargo flags are optional.
[group('build')]
run_c5 scenario="" args="": download_test_model (_run (c5_flags + " " + c5_runner) scenario args)

# Flash semantic-search (default) or home-assistant to the ESP32-C6 and open the serial monitor. Cargo flags are optional.
[group('build')]
run_c6 scenario="" args="": download_test_model (_run (c6_flags + " " + c6_runner) scenario args)

# Flash semantic-search (default) or home-assistant to the ESP32-P4 and open the serial monitor. Cargo flags are optional.
[group('build')]
run_p4 scenario="" args="": download_test_model (_run (p4_flags + " " + p4_runner) scenario args)

# Run one firmware binary: a known scenario selects its binary, Cargo flags are passed through.
[private]
_run chip_flags scenario args:
  cargo run --locked --release --package {{ example }} {{ chip_flags }} {{ if scenario == "" { "--bin " + example } else if scenario =~ '^(semantic-search|home-assistant)$' { "--bin " + scenario } else if scenario =~ '^-' { scenario } else { error("unknown scenario '" + scenario + "': use semantic-search or home-assistant") } }} {{ args }}

# Open the serial monitor without flashing.
[group('build')]
monitor args="":
  espflash monitor {{ args }}

# Remove Cargo build artifacts.
[group('build')]
[confirm("Are you sure you want to clean the build artifacts?")]
clean:
  cargo clean

# Refresh the workspace entries in Cargo.lock without upgrading dependencies.
[group('build')]
lock:
  cargo update --workspace
