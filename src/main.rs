use protocrap;
mod codegen;

use std::fs;
use std::io::{self, Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        return Ok(());
    }

    // Check for --embed mode
    let embed_idx = args.iter().position(|a| a == "--embed");
    if let Some(idx) = embed_idx {
        return run_embed_mode(&args, idx);
    }

    // Normal codegen mode
    run_codegen_mode(&args)
}

fn run_codegen_mode(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
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

fn run_embed_mode(args: &[String], embed_idx: usize) -> Result<(), Box<dyn std::error::Error>> {
    // Parse: protocrap <descriptor.pb> --embed <data.pb>:<type_name> [-o <output.pc.rs>] [--crate-path <path>]
    if args.len() < embed_idx + 2 {
        eprintln!("Error: --embed requires <data.pb>:<type_name> argument");
        return Err("missing embed argument".into());
    }

    let descriptor_path = &args[1];
    let embed_arg = &args[embed_idx + 1];

    // Parse data.pb:type_name
    let (data_path, type_name) = embed_arg.split_once(':').ok_or_else(|| {
        format!("Invalid --embed argument '{}': expected <data.pb>:<type_name>", embed_arg)
    })?;

    // Find output file (-o flag)
    let output_path = args.iter().position(|a| a == "-o").and_then(|i| args.get(i + 1));

    // Find crate path (--crate-path flag, defaults to "protocrap")
    let crate_path = args
        .iter()
        .position(|a| a == "--crate-path")
        .and_then(|i| args.get(i + 1).map(|s| s.as_str()))
        .unwrap_or("protocrap");

    // Detect JSON by file extension
    let is_json = data_path.ends_with(".json");

    // Read files
    let descriptor_bytes = fs::read(descriptor_path)?;
    let data_bytes = fs::read(data_path)?;

    eprintln!(
        "Embedding {} ({} bytes) as {} ({})",
        data_path,
        data_bytes.len(),
        type_name,
        if is_json { "JSON" } else { "binary" }
    );

    // Generate initializer
    let code = codegen::generate_embed(&descriptor_bytes, &data_bytes, type_name, is_json, crate_path)?;

    // Write output
    if let Some(path) = output_path {
        fs::write(path, &code)?;
        eprintln!("Generated {}", path);
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
    eprintln!("  {program} <descriptor.pb> --embed <data.pb>:<type> [-o output.pc.rs]");
    eprintln!();
    eprintln!("MODES:");
    eprintln!("  codegen   Generate Rust structs from proto descriptors (default)");
    eprintln!("  --embed   Generate const initializer from binary proto data");
    eprintln!();
    eprintln!("ARGUMENTS:");
    eprintln!("  descriptor.pb   FileDescriptorSet from protoc");
    eprintln!("  output.rs       Output Rust file (default: stdout)");
    eprintln!();
    eprintln!("EXAMPLES:");
    eprintln!("  # Generate Rust code from proto:");
    eprintln!("  protoc --descriptor_set_out=desc.pb --include_imports my.proto");
    eprintln!("  {program} desc.pb my.pc.rs");
    eprintln!();
    eprintln!("  # Embed binary proto as const:");
    eprintln!("  {program} desc.pb --embed config.pb:my.package.Config -o config.pc.rs");
    eprintln!("  # Then in Rust: const CONFIG: my::package::Config::ProtoType = include!(\"config.pc.rs\");");
}
