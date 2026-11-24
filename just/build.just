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

# Build the library for this computer with the standard Rust toolchain.
[group('build')]
build_host args="":
  {{ host_cargo }} build --locked {{ host_packages }} --target host-tuple {{ args }}

# Build the example firmware for the ESP32-S3 in release mode.
[group('build')]
build_s3 args="":
  cargo build --locked --release --package {{ example }} {{ args }}

# Build the example firmware for the ESP32-S31 in release mode.
[group('build')]
build_s31 args="":
  cargo build --locked --release --package {{ example }} {{ s31_flags }} {{ args }}

# Build the example firmware for both chips in release mode.
[group('build')]
build args="": (build_s3 args) (build_s31 args)

# Flash the example firmware to the ESP32-S3 and open the serial monitor.
[group('build')]
run_s3 args="":
  cargo run --locked --release --package {{ example }} {{ args }}

# Flash the example firmware to the ESP32-S31 and open the serial monitor.
[group('build')]
run_s31 args="":
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
