//! Checks the compiler diagnostic seen by a crate embedding an invalid model.

use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn invalid_header_points_at_the_folder_literal() {
    let consumer = tempfile::tempdir().unwrap();
    let root = consumer.path();
    let library_path = serde_json::to_string(env!("CARGO_MANIFEST_DIR")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"diagnostic-test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [dependencies]\nesp-local-agent = {{ path = {library_path} }}\n"
        ),
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    let source =
        "use esp_local_agent::{Model, model};\nstatic MODEL: Model = model!(\"broken\");\n";
    fs::write(root.join("src/lib.rs"), source).unwrap();
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

    let output = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "check",
            "--offline",
            "--message-format=json",
            "--target",
            "host-tuple",
            "--target-dir",
            "target",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter_map(|event| event.get("message").cloned())
        .find(|diagnostic| {
            diagnostic["message"].as_str().is_some_and(|message| {
                message.contains("model.safetensors: invalid header")
                    && message.contains("embedding")
                    && message.contains("expected a JSON object")
            })
        })
        .unwrap_or_else(|| panic!("{stdout}\n{stderr}"));
    let span = diagnostic["spans"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["is_primary"] == true)
        .unwrap();
    assert_eq!(
        Path::new(span["file_name"].as_str().unwrap()),
        Path::new("src/lib.rs")
    );
    let start = span["byte_start"].as_u64().unwrap() as usize;
    let end = span["byte_end"].as_u64().unwrap() as usize;
    assert_eq!(&source[start..end], "\"broken\"");
}
