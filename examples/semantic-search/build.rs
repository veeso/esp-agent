fn main() {
    require_model();
    linker_be_nice();
    configure_linker();
}

/// Stops the build with instructions when the downloaded model files are missing.
fn require_model() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
    );
    for file in ["model.safetensors", "tokenizer.json"] {
        let path = manifest_dir.join("model").join(file);
        println!("cargo:rerun-if-changed={path}", path = path.display());
        assert!(
            path.exists(),
            "The example model is missing: {path} does not exist.\n\n\
             Download it from the repository root with:\n\n    just download_test_model\n",
            path = path.display()
        );
    }
}

#[cfg(unix)]
fn check_xtensa_linker_available(target: &str) {
    println!("cargo:rerun-if-env-changed=PATH");

    println!(
        "cargo:rerun-if-env-changed={}",
        cargo_linker_env_var(target)
    );

    let linker = xtensa_linker(target);

    let error = match std::process::Command::new(&linker)
        .arg("--version")
        .output()
    {
        Ok(_) => return,
        Err(error) => error,
    };

    let export_file = std::env::var("HOME")
        .map(|home| format!("{home}/export-esp.sh"))
        .unwrap_or_else(|_| "$HOME/export-esp.sh".to_string());

    panic!(
        "Xtensa linker `{linker}` was not found in PATH or could not be executed: {error}.\n\n\
         Xtensa targets on Unix require espup's environment export file to be sourced before building.\n\
         Try: `source {export_file}`\n\n\
         If the export file does not exist, run `espup install` first.\n\
         For more details, see:\n\
         https://github.com/esp-rs/espup?tab=readme-ov-file#environment-variables-setup"
    );
}

#[cfg(not(unix))]
fn check_xtensa_linker_available(_: &str) {}

#[cfg(unix)]
fn xtensa_linker(target: &str) -> String {
    if let Some(linker) = cargo_configured_linker(target) {
        return linker;
    }

    target
        .strip_prefix("xtensa-")
        .and_then(|target| target.strip_suffix("-none-elf"))
        .map(|chip| format!("xtensa-{chip}-elf-gcc"))
        .unwrap_or_else(|| "xtensa-esp32-elf-gcc".to_string())
}

#[cfg(unix)]
fn cargo_configured_linker(target: &str) -> Option<String> {
    std::env::var(cargo_linker_env_var(target)).ok()
}

#[cfg(unix)]
fn cargo_linker_env_var(target: &str) -> String {
    let target = target.replace('-', "_").to_ascii_uppercase();
    format!("CARGO_TARGET_{target}_LINKER")
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script and you have included `use defmt_rtt as _;`"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos` or provided an external scheduler."
                    );
                    eprintln!();
                }
                "embedded_test_linker_file_not_added_to_rustflags" => {
                    eprintln!();
                    eprintln!(
                        "💡 `embedded-test` not found - make sure `embedded-test.x` is added as a linker script for tests"
                    );
                    eprintln!();
                }
                "free"
                | "malloc"
                | "calloc"
                | "get_free_internal_heap_size"
                | "malloc_internal"
                | "realloc_internal"
                | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            // we don't have anything helpful for "missing-lib" yet
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }
}

fn configure_linker() {
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for build scripts");
    let error_handling_script = std::env::current_exe().unwrap();

    if target.starts_with("xtensa-") {
        check_xtensa_linker_available(&target);
        println!(
            "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
            error_handling_script.display()
        );
        println!("cargo:rustc-link-arg=-Tlinkall.x");
    } else if target.starts_with("riscv32") {
        // The RISC-V chips use local copies of the esp-hal linker scripts that map 16 MB of
        // flash instead of 4 MB, so the embedded model fits. Two chips share each RISC-V target,
        // so the chip feature selects the folder.
        let chip = riscv_chip();
        let manifest_dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
        );
        let linker_dir = manifest_dir.join("ld").join(chip);
        let linker_script = linker_dir.join("linkall.x");

        println!("cargo:rerun-if-changed={}", linker_script.display());
        println!(
            "cargo:rerun-if-changed={}",
            linker_dir.join(format!("memory-{chip}.x")).display()
        );
        println!("cargo:rustc-link-search={}", linker_dir.display());
        println!(
            "cargo:rustc-link-arg=--error-handling-script={}",
            error_handling_script.display()
        );
        println!("cargo:rustc-link-arg=-T{}", linker_script.display());
    }
}

/// Returns the RISC-V chip selected through the chip feature of this crate.
fn riscv_chip() -> &'static str {
    const CHIPS: [&str; 4] = ["esp32c5", "esp32c6", "esp32p4", "esp32s31"];

    let enabled: Vec<&str> = CHIPS
        .into_iter()
        .filter(|chip| {
            let feature = format!("CARGO_FEATURE_{chip}", chip = chip.to_ascii_uppercase());
            std::env::var_os(feature).is_some()
        })
        .collect();
    match enabled.as_slice() {
        [chip] => chip,
        _ => panic!(
            "Enable exactly one RISC-V chip feature ({chips}) with --no-default-features.",
            chips = CHIPS.join(", ")
        ),
    }
}
