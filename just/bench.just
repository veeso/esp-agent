# Run every benchmark on this computer with the standard Rust toolchain.
[group('bench')]
bench_all args="": download_test_model
  {{ host_cargo }} bench --locked --package esp-agent-bench --benches --target host-tuple {{ args }}

# Run one benchmark group (`tokenize`, `embed`, or `search`) on this computer.
[group('bench')]
bench group args="": download_test_model
  {{ host_cargo }} bench --locked --package esp-agent-bench --target host-tuple --bench "{{ group }}" {{ args }}

# Run every benchmark once without measuring, to check that they work.
[group('bench')]
bench_smoke args="": download_test_model
  {{ host_cargo }} bench --locked --package esp-agent-bench --benches --target host-tuple {{ args }} -- --test

# Print the criterion results of the last run as a Markdown table.
[group('bench')]
bench_report dir="target/criterion": download_test_model
  {{ host_cargo }} run --locked --quiet --release --package esp-agent-bench --bin bench_report --target host-tuple -- "{{ dir }}"
