//! Prints the criterion results of the last benchmark run as a Markdown table.
//!
//! Usage: `bench_report [DIR]`, where `DIR` defaults to `target/criterion`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/criterion"), PathBuf::from);
    match esp_agent_bench::report::markdown(&dir) {
        Ok(table) => {
            print!("{table}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("bench_report: {error}");
            ExitCode::FAILURE
        }
    }
}
