#! /bin/bash
set -euo pipefail

BAZELISK=$(which bazelisk 2>/dev/null || echo "./bazelisk.sh")

echo "Generating protocrap descriptor... using Bazelisk at $BAZELISK"

# Check if we should use crates.io protocrap (for table layout changes)
CARGO_FLAGS="--features codegen"
if [ "${1:-}" = "bootstrap" ]; then
    echo "Using protocrap from crates.io for bootstrap"
    CARGO_FLAGS="--no-default-features --features bootstrap"
else
    echo "Using current protocrap"
fi

# Build descriptor set via Bazel
echo "Building descriptor.proto via Bazel..."
$BAZELISK build //:descriptor_set

# Run codegen on the generated descriptor set
RUST_BACKTRACE=full cargo run --bin protocrap $CARGO_FLAGS -- bazel-bin/descriptor_set.bin src/descriptor.pc.rs
