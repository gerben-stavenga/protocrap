# Protobuf Conformance Tests

This directory contains the conformance test setup for protocrap using Google's official protobuf conformance test suite.

## Running Tests

```bash
bazel test //conformance:conformance_test
```

To see test output and force re-run (bypass cache):

```bash
bazel test //conformance:conformance_test --nocache_test_results --test_output=all
```

## Protocol

The conformance test protocol works as follows:

1. Test runner writes to stdin: `[u32 length][ConformanceRequest bytes]`
2. Our binary reads request, executes test, writes to stdout: `[u32 length][ConformanceResponse bytes]`
3. Test runner validates response
4. Repeat for all test cases

## Files

- `failing_tests.txt` - Expected failures (by design or not yet implemented)
- `src/main.rs` - Conformance test binary implementation
- `BUILD.bazel` - Bazel rules for building and testing
