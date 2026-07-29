# Testing

`cargo test` runs the whole workspace. Don't use `--lib` — it only reaches `oas3-gen-support`.

## Fixtures

`main.rs` declares `fixtures/` as a `#[cfg(test)]` module, so `cargo test` compiles generated
output. Any change to generated code means regenerating, or the build breaks:

```bash
F=crates/oas3-gen/fixtures
cargo run -- generate client-mod -i $F/petstore.json           -o $F/petstore           --enable-builders --all-schemas --all-headers
cargo run -- generate server-mod -i $F/petstore.json           -o $F/petstore_server    --enable-builders --all-schemas --all-headers
cargo run -- generate client-mod -i $F/union_serde.json        -o $F/union_serde        --enable-builders --all-schemas
cargo run -- generate client-mod -i $F/intersection_union.json -o $F/intersection_union --enable-builders --all-schemas
cargo run -- generate client-mod -i $F/event_stream.json       -o $F/event_stream       --enable-builders --all-schemas
```

Flags differ per fixture — copy the line, don't improvise. `event_stream` is *not* in the
`#[cfg(test)]` module, so nothing compile-checks it; review its diff by eye.

The fixture diff is how generated-output changes get reviewed. Read it before claiming a change
is correct.

## Requirements

- Unit tests in `#[cfg(test)]` modules alongside the code; integration tests in `src/tests/`.
- Cover happy path, boundaries (empty, special characters), and error cases.
- Test behavior generally. Never hard-code a value or special-case an assertion to make a test
  pass — if a test or requirement looks wrong, say so.
- `cargo test` plus the fixture diff is the verification step. Nothing to add on top.

## Style

Table-driven: an array of `(input, expected)` cases iterated in one test, with the input in the
assertion message. Prefer few comprehensive tests over many single-assertion ones.

```rust
#[test]
fn test_normalize_numbers() {
  let cases = [(json!(404), "Value404", "404"), (json!(-42), "Value-42", "-42")];
  for (val, name, rename) in cases {
    let res = normalize(&val).unwrap();
    assert_eq!(res.name, name, "name mismatch for {val:?}");
    assert_eq!(res.rename_value, rename, "rename mismatch for {val:?}");
  }
}
```

## Debugging

- Inspect with `tracing`/`dbg!()` or a throwaway test that exercises the hypothesis.
- `RUST_BACKTRACE=1` for traces; `cargo-expand` for macro and derive output.
- Symptoms in generated output usually originate one or two pipeline stages upstream of where
  they appear — see [architecture.md](architecture.md#pipeline).
- Find the mechanism before editing. Pick an approach and commit; revisit only on contradicting
  evidence.
- Delete scratch files and temporary scripts before finishing.

## Coverage

```bash
cargo tarpaulin --bins --skip-clean -o Markdown   # writes tarpaulin-report.md; delete when done
```
