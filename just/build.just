# Example firmware crate that the build and run recipes target.
example := "semantic-search"

# Cargo flag that selects the ESP32-S31 target.
s31_target := "--target riscv32imafc-unknown-none-elf"

# Cargo flags that select the ESP32-S31 target and the example chip feature.
s31_flags := s31_target + " --no-default-features --features " + example + "/esp32s31"

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
