// build.rs

use std::io::Result;

use protocrap_codegen::generate_proto;

fn main() -> Result<()> {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    generate_proto(&out_dir, "proto/descriptor.proto", "descriptor.pc.rs").map_err(|e| {
        // anyhow::Error implements std::error::Error
        std::io::Error::new(std::io::ErrorKind::Other, e)
    })?;

    Ok(())
}
