#! /bin/bash
set -euo pipefail

BAZELISK=$(which bazelisk || echo "bazelisk")

echo "Generating protocrap descriptor... using Bazelisk at $BAZELISK"

# Check if we should use protocrap_stable (for table layout changes)
CARGO_FLAGS=""
if [ "${1:-}" = "bootcrap" ]; then
    echo "Using protocrap_stable for bootstrap"
    CARGO_FLAGS="--no-default-features --features bootcrap"
else
    echo "Using current protocrap"
fi

# Build the codegen tool first (prevents rebuild during run)
cargo build -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS

# Build descriptor set via Bazel
echo "Building descriptor.proto via Bazel..."
$BAZELISK build //:descriptor_set

# Run codegen on the generated descriptor set
RUST_BACKTRACE=full cargo run -p protocrap-codegen --bin protocrap-codegen $CARGO_FLAGS -- bazel-bin/descriptor.bin src/descriptor.pc.rs
