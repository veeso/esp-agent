alias lint := clippy

# Format Markdown, Rust, TOML, and YAML files.
[group('code-check')]
fmt args="":
  dprint fmt {{ args }}

# Check formatting without changing files.
[group('code-check')]
fmt_check args="":
  dprint check {{ args }}

# Run Clippy for the ESP32-S3.
[group('code-check')]
clippy_s3 args="": download_test_model
  cargo clippy --locked --workspace --exclude esp-agent-bench {{ args }}

# Run Clippy for the ESP32-S31.
[group('code-check')]
clippy_s31 args="": download_test_model
  cargo clippy --locked --workspace --exclude esp-agent-bench {{ s31_flags }} {{ args }}

# Run Clippy for the ESP32-C5.
[group('code-check')]
clippy_c5 args="": download_test_model
  cargo clippy --locked --workspace --exclude esp-agent-bench {{ c5_flags }} {{ args }}

# Run Clippy for the ESP32-C6.
[group('code-check')]
clippy_c6 args="": download_test_model
  cargo clippy --locked --workspace --exclude esp-agent-bench {{ c6_flags }} {{ args }}

# Run Clippy for the ESP32-P4.
[group('code-check')]
clippy_p4 args="": download_test_model
  cargo clippy --locked --workspace --exclude esp-agent-bench {{ p4_flags }} {{ args }}

# Run Clippy for every supported chip.
[group('code-check')]
clippy args="": (clippy_s3 args) (clippy_s31 args) (clippy_c5 args) (clippy_c6 args) (clippy_p4 args)

# Run Clippy for the benchmarks on this computer with the standard Rust toolchain.
[group('code-check')]
clippy_bench args="": download_test_model
  {{ host_cargo }} clippy --locked --package esp-agent-bench --all-targets --target host-tuple {{ args }}

# Build the library documentation for the ESP32-S3 and deny warnings.
[group('code-check')]
doc_s3 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ args }}

# Build the library documentation for the ESP32-S31 and deny warnings.
[group('code-check')]
doc_s31 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ s31_target }} {{ args }}

# Build the library documentation for the ESP32-C5 and deny warnings.
[group('code-check')]
doc_c5 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ c5_target }} {{ args }}

# Build the library documentation for the ESP32-C6 and deny warnings.
[group('code-check')]
doc_c6 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ c6_target }} {{ args }}

# Build the library documentation for the ESP32-P4 and deny warnings.
[group('code-check')]
doc_p4 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ p4_target }} {{ args }}

# Build the library documentation for every supported chip and deny warnings.
[group('code-check')]
doc args="": (doc_s3 args) (doc_s31 args) (doc_c5 args) (doc_c6 args) (doc_p4 args)

# Run the library tests on this computer with the standard Rust toolchain.
[group('code-check')]
test args="": download_test_model
  {{ host_cargo }} test --locked {{ host_packages }} --target host-tuple {{ args }}

# Run the library tests on this computer with coverage and write an LCOV report.
# cargo-llvm-cov does not accept `host-tuple`, so pass the host triple explicitly.
[group('code-check')]
coverage output="lcov.info" args="": download_test_model
  {{ host_cargo }} llvm-cov test --locked {{ host_packages }} --target "$(rustc +{{ host_toolchain }} --print host-tuple)" --lcov --output-path {{ output }} {{ args }}

# Run the benchmark report tests on this computer with the standard Rust toolchain.
[group('code-check')]
test_bench args="": download_test_model
  {{ host_cargo }} test --locked --package esp-agent-bench --lib --bins --target host-tuple {{ args }}

# Check advisories, licenses, bans, and dependency sources.
[group('code-check')]
deny args="":
  cargo deny --workspace check {{ args }}

# Scan paths for verified or unknown secrets.
[group('code-check')]
scan_secrets *args=".":
  trufflehog filesystem {{ args }} --results=verified,unknown --fail --fail-on-scan-errors --no-update

# Point Git at the tracked hook directory.
[group('code-check')]
setup_githooks:
  git config core.hooksPath .githooks
  @echo "git hooks installed: core.hooksPath = .githooks"

# Run the complete local quality gate.
[group('code-check')]
check:
  just fmt_check
  just clippy "-- -D warnings"
  just doc
  just build_host
  just clippy_bench "-- -D warnings"
  just test
  just test_bench
  just deny
