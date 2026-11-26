//! Checks macro expansion in downstream Cargo crates.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

#[test]
fn downstream_macro_expansion() {
    let suite = tempfile::tempdir().unwrap();
    let root = suite.path();
    // Keep consumer sources isolated while reusing their dependency builds.
    let target = root.join("target");
    invalid_header_points_at_the_folder_literal(&root.join("invalid-model"), &target);
    vocabulary_previous_target_error_points_at_the_phrase_literal(
        &root.join("invalid-vocabulary"),
        &target,
    );
    vocabulary_compiles_through_a_reexport_without_a_direct_dependency(
        &root.join("vocabulary-reexport"),
        &target,
    );
    suite.close().expect("remove downstream test directory");
}

fn check_consumer(root: &Path, target: &Path) -> Output {
    Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "check",
            "--offline",
            "--message-format=json",
            "--target",
            "host-tuple",
            "--target-dir",
        ])
        .arg(target)
        .output()
        .unwrap()
}

fn write_consumer(root: &Path, source: &str) {
    let library_path = serde_json::to_string(env!("CARGO_MANIFEST_DIR")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"diagnostic-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [dependencies]\nesp-agent = {{ path = {library_path} }}\n"
        ),
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), source).unwrap();
}

fn assert_diagnostic_at(
    root: &Path,
    target: &Path,
    source: &str,
    message_parts: &[&str],
    highlighted: &str,
) {
    let output = check_consumer(root, target);
    let case = root.display();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected a compilation error in {case}\n{stdout}\n{stderr}"
    );
    let diagnostic = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter_map(|event| event.get("message").cloned())
        .find(|diagnostic| {
            diagnostic["message"].as_str().is_some_and(|message| {
                diagnostic["level"] == "error"
                    && message_parts.iter().all(|part| message.contains(part))
            })
        })
        .unwrap_or_else(|| panic!("missing expected diagnostic in {case}\n{stdout}\n{stderr}"));
    let span = diagnostic["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["is_primary"] == true)
        .unwrap_or_else(|| panic!("missing primary span in {case}\n{diagnostic}"));
    assert_eq!(
        Path::new(span["file_name"].as_str().unwrap()),
        Path::new("src/lib.rs"),
        "{case}"
    );
    let start = span["byte_start"].as_u64().unwrap() as usize;
    let end = span["byte_end"].as_u64().unwrap() as usize;
    assert_eq!(&source[start..end], highlighted, "{case}");
}

fn invalid_header_points_at_the_folder_literal(root: &Path, target: &Path) {
    let source = "use esp_agent::{Model, model};\nstatic MODEL: Model = model!(\"broken\");\n";
    write_consumer(root, source);
    let folder = root.join("broken");
    fs::create_dir(&folder).unwrap();
    fs::write(
        folder.join("tokenizer.json"),
        include_bytes!("fixtures/tiny-2d/tokenizer.json"),
    )
    .unwrap();
    let header = br#"{"embedding":4}"#;
    let mut file = (header.len() as u64).to_le_bytes().to_vec();
    file.extend_from_slice(header);
    fs::write(folder.join("model.safetensors"), file).unwrap();

    assert_diagnostic_at(
        root,
        target,
        source,
        &[
            "model.safetensors: invalid header",
            "embedding",
            "expected a JSON object",
        ],
        "\"broken\"",
    );
}

fn vocabulary_previous_target_error_points_at_the_phrase_literal(root: &Path, target: &Path) {
    let source = r#"use esp_agent::{Vocabulary, vocabulary};
enum Action { TurnOff }
static ITALIAN: Vocabulary<Action> = vocabulary! {
    Action::TurnOff => previous ["spegni {target}"];
};
"#;
    write_consumer(root, source);
    assert_diagnostic_at(
        root,
        target,
        source,
        &["cannot hold {target}"],
        r#""spegni {target}""#,
    );
}

fn vocabulary_compiles_through_a_reexport_without_a_direct_dependency(root: &Path, target: &Path) {
    let library_path = serde_json::to_string(env!("CARGO_MANIFEST_DIR")).unwrap();
    fs::create_dir_all(root.join("sdk/src")).unwrap();
    fs::write(
        root.join("sdk/Cargo.toml"),
        format!(
            "[package]\nname = \"sdk\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [dependencies]\nesp-agent = {{ path = {library_path} }}\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("sdk/src/lib.rs"),
        "#![no_std]\npub use esp_agent::{Vocabulary, vocabulary};\n",
    )
    .unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"firmware\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
         [dependencies]\nsdk = { path = \"sdk\" }\n",
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"#![no_std]
use sdk::{Vocabulary, vocabulary};

pub enum Action { TurnOff }

pub static ITALIAN: Vocabulary<Action> = vocabulary! {
    Action::TurnOff => previous ["spegnila"];
};
"#,
    )
    .unwrap();

    let output = check_consumer(root, target);
    let case = root.display();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{case}\n{stdout}\n{stderr}");
}
