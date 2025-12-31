# Benchmarks

Compares protocrap encoding/decoding performance against prost.

## Running with Cargo (includes prost comparison)

```bash
# Full benchmark
cargo bench -p protocrap-benchmarks

# Quick test run
cargo bench -p protocrap-benchmarks -- --test

# Specific benchmark
cargo bench -p protocrap-benchmarks -- decode
```

## Running with Bazel (protocrap only)

```bash
# Full benchmark (use -c opt for release optimizations)
bazel run -c opt //benchmark:bench

# Quick test run
bazel run -c opt //benchmark:bench -- --test

# Specific benchmark
bazel run -c opt //benchmark:bench -- decode
```
