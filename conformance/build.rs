use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=conformance_all.proto");
    println!("cargo:rerun-if-changed=conformance.proto");
    println!("cargo:rerun-if-changed=test_messages_proto2.proto");
    println!("cargo:rerun-if-changed=test_messages_proto3.proto");

    let out_dir = std::env::var("OUT_DIR").unwrap();

    // Generate descriptor set from wrapper proto - protoc will deduplicate imports
    let desc_file = format!("{}/conformance_all.bin", out_dir);
    let status = Command::new("protoc")
        .args(&[
            "--include_imports",
            "--descriptor_set_out",
            &desc_file,
            "conformance_all.proto",
        ])
        .status()
        .expect("Failed to run protoc");

    if !status.success() {
        panic!("protoc failed");
    }

    // Find the codegen binary - use the already-built one instead of cargo run
    let codegen_bin = if let Ok(path) = std::env::var("PROTOCRAP_CODEGEN") {
        PathBuf::from(path)
    } else {
        // Try to find it in the workspace target directory
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let workspace_root = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
        let debug_bin = workspace_root.join("target/debug/protocrap-codegen");
        let release_bin = workspace_root.join("target/release/protocrap-codegen");

        if release_bin.exists() {
            release_bin
        } else if debug_bin.exists() {
            debug_bin
        } else {
            panic!(
                "protocrap-codegen binary not found. Please build it first with: cargo build -p protocrap-codegen"
            );
        }
    };

    println!(
        "cargo:warning=Using codegen binary: {}",
        codegen_bin.display()
    );

    // Generate Rust code
    let out_file = format!("{}/conformance_all.pc.rs", out_dir);
    let status = Command::new(&codegen_bin)
        .args(&[&desc_file, &out_file])
        .status()
        .expect("Failed to run protocrap-codegen");

    if !status.success() {
        panic!("protocrap-codegen failed");
    }

    println!("cargo:warning=Generated {} from {}", out_file, desc_file);
}
