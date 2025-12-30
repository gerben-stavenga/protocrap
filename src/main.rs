mod codegen;

use std::fs;
use std::io::{self, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    // Read descriptor bytes
    let descriptor_bytes = if args[1] == "-" {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        fs::read(&args[1])?
    };

    eprintln!("Read descriptor ({} bytes)", descriptor_bytes.len());

    // Generate code
    let code = codegen::generate(&descriptor_bytes)?;

    // Write output
    if args.len() > 2 {
        fs::write(&args[2], &code)?;
        eprintln!("Generated {}", args[2]);
    } else {
        io::stdout().write_all(code.as_bytes())?;
    }

    Ok(())
}

fn print_usage(program: &str) {
    eprintln!("Protocrap Code Generator");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  {program} <descriptor.pb> [output.rs]");
    eprintln!("  {program} - < descriptor.pb > output.rs");
    eprintln!();
    eprintln!("ARGUMENTS:");
    eprintln!("  descriptor.pb   FileDescriptorSet from protoc");
    eprintln!("  output.rs       Output Rust file (default: stdout)");
    eprintln!();
    eprintln!("EXAMPLE:");
    eprintln!("  protoc --descriptor_set_out=desc.pb --include_imports my.proto");
    eprintln!("  {program} desc.pb my.pc.rs");
}
