#! /bin/bash
set -euo pipefail

BAZELISK=$(which bazelisk 2>/dev/null || echo "./bazelisk.sh")

echo "Generating protocrap descriptor... using Bazelisk at $BAZELISK"

# Check if we should use crates.io protocrap (for table layout changes)
if [ "${1:-}" = "bootstrap" ]; then
    echo "Using protocrap from crates.io for bootstrap"
    CODEGEN_TARGET="//:protocrap-codegen-bootstrap"
else
    echo "Using current protocrap"
    CODEGEN_TARGET="//:protocrap-codegen"
fi

# Build descriptor set and run codegen
echo "Building descriptor.proto via Bazel..."
$BAZELISK build //:descriptor_set $CODEGEN_TARGET

# Run codegen on the generated descriptor set
bazel-bin/${CODEGEN_TARGET#//:} bazel-bin/descriptor_set.bin src/descriptor.pc.rs
