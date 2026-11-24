//! Renders the results that criterion saved on disk as a Markdown table.

use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The part of `new/benchmark.json` that the report reads.
#[derive(Deserialize)]
struct Benchmark {
    full_id: String,
}

/// The part of `new/estimates.json` that the report reads.
#[derive(Deserialize)]
struct Estimates {
    mean: Estimate,
}

#[derive(Deserialize)]
struct Estimate {
    point_estimate: f64,
    confidence_interval: ConfidenceInterval,
}

#[derive(Deserialize)]
struct ConfidenceInterval {
    lower_bound: f64,
    upper_bound: f64,
}

/// Formats a duration in nanoseconds with two decimals and the largest fitting unit.
#[must_use]
pub fn format_ns(ns: f64) -> String {
    let (value, unit) = if ns < 1e3 {
        (ns, "ns")
    } else if ns < 1e6 {
        (ns / 1e3, "µs")
    } else if ns < 1e9 {
        (ns / 1e6, "ms")
    } else {
        (ns / 1e9, "s")
    };
    format!("{value:.2} {unit}")
}

/// Collects the directories named `new` below `dir` that hold a `benchmark.json`.
fn collect_results(dir: &Path, found: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|name| name == "new")
            && path.join("benchmark.json").is_file()
        {
            found.push(path);
        } else {
            collect_results(&path, found)?;
        }
    }
    Ok(())
}

fn invalid(path: &Path, error: serde_json::Error) -> Error {
    let path = path.display();
    Error::new(ErrorKind::InvalidData, format!("{path}: {error}"))
}

/// Reads the results of the last run below `criterion_dir` into a Markdown table sorted by
/// benchmark id.
///
/// # Errors
///
/// Fails with [`ErrorKind::NotFound`] when `criterion_dir` holds no benchmark, and with the
/// underlying error when the directory cannot be read or a JSON file does not parse.
pub fn markdown(criterion_dir: &Path) -> Result<String> {
    let mut dirs = Vec::new();
    collect_results(criterion_dir, &mut dirs)?;
    if dirs.is_empty() {
        return Err(Error::new(
            ErrorKind::NotFound,
            format!(
                "no benchmark results in {path}",
                path = criterion_dir.display()
            ),
        ));
    }
    let mut rows = Vec::new();
    for dir in dirs {
        let benchmark_path = dir.join("benchmark.json");
        let benchmark: Benchmark = serde_json::from_slice(&fs::read(&benchmark_path)?)
            .map_err(|error| invalid(&benchmark_path, error))?;
        let estimates_path = dir.join("estimates.json");
        let estimates: Estimates = serde_json::from_slice(&fs::read(&estimates_path)?)
            .map_err(|error| invalid(&estimates_path, error))?;
        let mean = estimates.mean;
        let interval = &mean.confidence_interval;
        let (point, lower, upper) = (
            format_ns(mean.point_estimate),
            format_ns(interval.lower_bound),
            format_ns(interval.upper_bound),
        );
        rows.push((benchmark.full_id, format!("{point} | {lower} to {upper}")));
    }
    rows.sort();
    let mut table = String::from("| Benchmark | Mean | 95% interval |\n| --- | ---: | ---: |\n");
    for (id, cells) in rows {
        table.push_str(&format!("| {id} | {cells} |\n"));
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::ErrorKind;
    use std::path::Path;

    use super::{format_ns, markdown};

    /// Writes the two files that criterion keeps for one benchmark, with only the fields the
    /// report reads.
    fn write_benchmark(root: &Path, dir: &str, full_id: &str, mean: f64, lower: f64, upper: f64) {
        let new = root.join(dir).join("new");
        fs::create_dir_all(&new).unwrap();
        fs::write(
            new.join("benchmark.json"),
            format!(r#"{{"full_id": "{full_id}"}}"#),
        )
        .unwrap();
        fs::write(
            new.join("estimates.json"),
            format!(
                r#"{{"mean": {{"point_estimate": {mean}, "confidence_interval": {{"lower_bound": {lower}, "upper_bound": {upper}}}}}}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn format_ns_picks_the_unit_at_each_boundary() {
        assert_eq!(format_ns(999.0), "999.00 ns");
        assert_eq!(format_ns(1_000.0), "1.00 µs");
        assert_eq!(format_ns(1_234_567.0), "1.23 ms");
        assert_eq!(format_ns(2_500_000_000.0), "2.50 s");
    }

    #[test]
    fn markdown_lists_new_results_sorted_by_id() {
        let root = tempfile::tempdir().unwrap();
        // Directory names sort opposite to the ids, so sorting by path fails this test.
        write_benchmark(
            root.path(),
            "a_search",
            "search/top3/1024",
            2.5e6,
            2.4e6,
            2.6e6,
        );
        write_benchmark(
            root.path(),
            "z_embed",
            "embed/embed/sentence",
            1_234.0,
            1_210.0,
            1_250.0,
        );
        // A previous run and the HTML report must be ignored.
        let base = root.path().join("z_embed/base");
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("benchmark.json"), r#"{"full_id": "stale"}"#).unwrap();
        fs::create_dir_all(root.path().join("report")).unwrap();
        fs::write(root.path().join("report/index.html"), "<html></html>").unwrap();

        assert_eq!(
            markdown(root.path()).unwrap(),
            "| Benchmark | Mean | 95% interval |\n\
             | --- | ---: | ---: |\n\
             | embed/embed/sentence | 1.23 µs | 1.21 µs to 1.25 µs |\n\
             | search/top3/1024 | 2.50 ms | 2.40 ms to 2.60 ms |\n"
        );
    }

    #[test]
    fn markdown_fails_without_results() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            markdown(root.path()).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert!(markdown(&root.path().join("missing")).is_err());
    }
}
