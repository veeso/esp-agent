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
clippy_s3 args="":
  cargo clippy --locked --workspace {{ args }}

# Run Clippy for the ESP32-S31.
[group('code-check')]
clippy_s31 args="":
  cargo clippy --locked --workspace {{ s31_flags }} {{ args }}

# Run Clippy for both chips.
[group('code-check')]
clippy args="": (clippy_s3 args) (clippy_s31 args)

# Build the library documentation for the ESP32-S3 and deny warnings.
[group('code-check')]
doc_s3 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ args }}

# Build the library documentation for the ESP32-S31 and deny warnings.
[group('code-check')]
doc_s31 args="":
  RUSTDOCFLAGS="-D warnings" cargo doc --locked --release --no-deps --package esp-agent --package esp-agent-macros {{ s31_target }} {{ args }}

# Build the library documentation for both chips and deny warnings.
[group('code-check')]
doc args="": (doc_s3 args) (doc_s31 args)

# Run the library tests on the host, rebuilding std for the host target.
[group('code-check')]
test args="":
  cargo test --locked --package esp-agent --package esp-agent-macros --target "$(rustc -vV | sed -n 's/^host: //p')" --config 'unstable.build-std=["std","test","panic_unwind"]' {{ args }}

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
  just test
  just deny
