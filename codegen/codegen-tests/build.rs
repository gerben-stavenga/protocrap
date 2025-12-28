// build.rs

use std::io::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = PathBuf::from(&manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    // Find descriptor set - either from env var or default bazel-bin location
    let desc_file = if let Ok(path) = std::env::var("PROTOCRAP_DESCRIPTOR_SET") {
        println!("cargo:rerun-if-env-changed=PROTOCRAP_DESCRIPTOR_SET");
        PathBuf::from(path)
    } else {
        let default_path =
            workspace_root.join("bazel-bin/codegen/codegen-tests/test_descriptor_set.bin");
        println!("cargo:rerun-if-changed={}", default_path.display());
        default_path
    };

    if !desc_file.exists() {
        panic!(
            "Descriptor set not found at {}. Build it first with: bazel build //codegen/codegen-tests:test_descriptor_set",
            desc_file.display()
        );
    }

    // Find the codegen binary
    let codegen_bin = if let Ok(path) = std::env::var("PROTOCRAP_CODEGEN") {
        PathBuf::from(path)
    } else {
        let debug_bin = workspace_root.join("target/debug/protocrap-codegen");
        let release_bin = workspace_root.join("target/release/protocrap-codegen");

        if release_bin.exists() {
            release_bin
        } else if debug_bin.exists() {
            debug_bin
        } else {
            panic!(
                "protocrap-codegen binary not found. Build it first with: cargo build -p protocrap-codegen"
            );
        }
    };

    println!("cargo:rerun-if-changed={}", codegen_bin.display());

    // Generate Rust code
    let out_file = format!("{}/test.pc.rs", out_dir);
    let status = std::process::Command::new(&codegen_bin)
        .args([desc_file.to_str().unwrap(), &out_file])
        .status()
        .expect("Failed to run protocrap-codegen");

    if !status.success() {
        panic!("protocrap-codegen failed");
    }

    Ok(())
}
