use std::io::Result;

fn main() -> Result<()> {
    // Print out the OUT_DIR for debugging
    println!(
        "cargo:warning=OUT_DIR={}",
        std::env::var("OUT_DIR").unwrap()
    );

    prost_build::Config::new()
        .out_dir(std::env::var("OUT_DIR").unwrap())
        .compile_protos(&["proto/test.proto"], &["proto/"])?;

    // List what was generated
    for entry in std::fs::read_dir(std::env::var("OUT_DIR").unwrap())? {
        println!("cargo:warning=Generated: {:?}", entry?.path());
    }

    Ok(())
}
