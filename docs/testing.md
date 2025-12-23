# Testing

## Running Tests

```bash
# Run all tests (tests both oas3-gen and oas3-gen-support crates)
cargo test
```

Note: Use `cargo test` without `--lib` to test the entire workspace. Using `cargo test --lib` only tests the oas3-gen-support library crate.

## Rebuilding Fixtures

All code changes that affect code generation output require rebuilding the fixture files. Run these commands after making changes:

```bash
# Rebuild petstore fixture (types.rs, client.rs, mod.rs)
cargo run -- generate client-mod -i crates/oas3-gen/fixtures/petstore.json -o crates/oas3-gen/fixtures/petstore --all-schemas

# Rebuild union_serde fixture (types.rs, client.rs, mod.rs)
cargo run -- generate client-mod -i crates/oas3-gen/fixtures/union_serde.json -o crates/oas3-gen/fixtures/union_serde --all-schemas
```

## Code Coverage

```bash
# Generate code coverage report in Markdown format
cargo tarpaulin --bins --skip-clean -o Markdown
```

This command generates a `tarpaulin-report.md` file with detailed coverage statistics. View the report to identify untested code paths, then delete the file when finished.

## Test Requirements

- All code changes require unit tests in `#[cfg(test)]` modules
- Cover: happy paths, edge cases (empty/boundary/special chars), error conditions
- Run `cargo test` before committing

## Test Style

- Use table-driven tests: Group related cases into arrays of `(input, expected)` tuples and iterate with descriptive assertions
- Consolidate by logical grouping: Combine tests that exercise the same function with different inputs into a single test
- Prefer fewer comprehensive tests over many trivial single-assertion tests
- Extract helper functions (e.g., `make_variant()`) to reduce boilerplate in test setup
- Include context in assertion messages: `assert_eq!(result, expected, "failed for {input:?}")`

### Example

```rust
#[test]
fn test_normalize_numbers() {
  let cases = [
    (json!(404), "Value404", "404"),
    (json!(-42), "Value-42", "-42"),
    (json!(0), "Value0", "0"),
  ];
  for (val, expected_name, expected_rename) in cases {
    let res = normalize(&val).unwrap();
    assert_eq!(res.name, expected_name, "name mismatch for {val:?}");
    assert_eq!(res.rename_value, expected_rename, "rename mismatch for {val:?}");
  }
}
```

## Debugging

When debugging issues in this project, follow these principles:

- Use logging (tracing, log) or macros like `dbg!()` to inspect state
- Make code changes only if you have high confidence they can solve the problem
- When debugging, try to determine the root cause rather than addressing symptoms
- Debug for as long as needed to identify the root cause and identify a fix
- Use print statements, logs, or temporary code to inspect program state, including descriptive statements or error messages to understand what's happening
- To test hypotheses, you can also add test statements or functions
- Revisit your assumptions if unexpected behavior occurs
- Use `RUST_BACKTRACE=1` to get stack traces, and `cargo-expand` to debug macros and derive logic
- Read terminal output
