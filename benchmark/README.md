# Benchmarks

Compares protocrap encoding/decoding performance against prost.

## Running

```bash
# Build optimized benchmark binary
bazel build -c opt //benchmark:bench

# Run full benchmark
./bazel-bin/benchmark/bench

# Quick test run (verify benchmarks work)
./bazel-bin/benchmark/bench --test

# Specific benchmark group
./bazel-bin/benchmark/bench decode
./bazel-bin/benchmark/bench encode
```

Note: Run the binary directly instead of `bazel run` to get actual benchmark measurements (Criterion needs a TTY).
