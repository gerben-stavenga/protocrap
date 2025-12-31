#!/bin/bash
set -e

GENERATED="$TEST_SRCDIR/_main/descriptor.pc.rs.generated"
CHECKED_IN="$TEST_SRCDIR/_main/src/descriptor.pc.rs"

if diff -q "$GENERATED" "$CHECKED_IN" > /dev/null; then
    echo "descriptor.pc.rs is up to date"
    exit 0
else
    echo "ERROR: descriptor.pc.rs is out of date!"
    echo "Run ./generate_descriptor.sh to regenerate it."
    echo ""
    echo "Diff (first 50 lines):"
    diff "$GENERATED" "$CHECKED_IN" | head -50 || true
    exit 1
fi
