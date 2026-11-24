# Example firmware crate that the build and run recipes target.
example := "semantic-search"

# Cargo flag that selects the ESP32-S31 target.
s31_target := "--target riscv32imafc-unknown-none-elf"

# Cargo flags that select the ESP32-S31 target and the example chip feature.
s31_flags := s31_target + " --no-default-features --features " + example + "/esp32s31"

# Standard Rust toolchain for regular computers; keep it equal to `rust-version`.
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

# Build the example firmware for both chips in release mode.
[group('build')]
build args="": (build_s3 args) (build_s31 args)

# Flash the example firmware to the ESP32-S3 and open the serial monitor.
[group('build')]
run_s3 args="": download_test_model
  cargo run --locked --release --package {{ example }} {{ args }}

# Flash the example firmware to the ESP32-S31 and open the serial monitor.
[group('build')]
run_s31 args="": download_test_model
  cargo run --locked --release --package {{ example }} {{ s31_flags }} {{ args }}

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
