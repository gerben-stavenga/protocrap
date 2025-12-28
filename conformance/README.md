# Protobuf Conformance Tests

This directory contains the conformance test setup for protocrap using Google's official protobuf conformance test suite.

## Running Tests

The simplest way to run the conformance tests:

```bash
bazel test //conformance:conformance_test
```

To see test output and force re-run (bypass cache):

```bash
bazel test //conformance:conformance_test --nocache_test_results --test_output=all
```

This will:
1. Build the FileDescriptorSet from protobuf's conformance protos
2. Build the Rust conformance binary via cargo
3. Run the conformance test suite with the expected failure list

## Manual Testing

If you prefer to run steps manually:

```bash
# Build the descriptor set
bazel build //conformance:conformance_descriptor_set

# Build the conformance binary
cargo build -p protocrap-conformance

# Run tests with failure list
bazel run @protobuf//conformance:conformance_test_runner -- \
    --enforce_recommended \
    --failure_list $PWD/conformance/failing_tests.txt \
    $PWD/target/debug/conformance-protocrap
```

## Protocol

The conformance test protocol works as follows:

1. Test runner writes to stdin: `[u32 length][ConformanceRequest bytes]`
2. Our binary reads request, executes test, writes to stdout: `[u32 length][ConformanceResponse bytes]`
3. Test runner validates response
4. Repeat for all test cases

## Files

- `failing_tests.txt` - Expected failures (by design or not yet implemented)
- `build.rs` - Generates Rust code from the FileDescriptorSet
- `src/main.rs` - Conformance test binary implementation
- `BUILD.bazel` - Bazel rules for building and testing
