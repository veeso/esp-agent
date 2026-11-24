# Cargo flags that select the ESP32-S31 target and feature.
s31_flags := "--target riscv32imafc-unknown-none-elf --no-default-features --features esp32s31"

# Build the firmware for the ESP32-S3 in release mode.
[group('build')]
build_s3 args="":
  cargo build --locked --release {{ args }}

# Build the firmware for the ESP32-S31 in release mode.
[group('build')]
build_s31 args="":
  cargo build --locked --release {{ s31_flags }} {{ args }}

# Build the firmware for both chips in release mode.
[group('build')]
build args="": (build_s3 args) (build_s31 args)

# Flash the ESP32-S3 and open the serial monitor.
[group('build')]
run_s3 args="":
  cargo run --locked --release {{ args }}

# Flash the ESP32-S31 and open the serial monitor.
[group('build')]
run_s31 args="":
  cargo run --locked --release {{ s31_flags }} {{ args }}

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
