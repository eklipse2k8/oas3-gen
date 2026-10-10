# Testing

`cargo test` runs the whole workspace. Don't use `--lib` — it only reaches `oas3-gen-support`.

## Fixtures

One document, `crates/oas3-gen/fixtures/petstore.json`, is the fixture: an OpenAPI 3.1.2 pet
store that exercises every feature the generator supports. It generates two crates with
`--workspace`, `fixtures/petstore-client` and `fixtures/petstore-server`. Both are workspace
members, so `cargo build` compiles generated output against its own generated `Cargo.toml`. Any
change to generated code means regenerating, or the build breaks:

```bash
F=crates/oas3-gen/fixtures
cargo run -- generate client-mod -i $F/petstore.json -o $F/petstore-client --workspace --enable-builders --all-schemas --all-headers
cargo run -- generate server-mod -i $F/petstore.json -o $F/petstore-server --workspace --enable-builders --all-schemas --all-headers

cargo clippy --fix --allow-dirty --all --all-targets -- -W clippy::pedantic
cargo +nightly fmt --all
```

Committed fixtures are post-clippy and post-rustfmt, so the last two commands are part of
regenerating, not an afterthought — skip them and the diff fills with lint and formatting noise.

The fixture diff is how generated-output changes get reviewed. Read it before claiming a change
is correct.

A feature the generator gains belongs in `petstore.json`, so the generated client and server
exercise it. A spec that only feeds one converter test, especially one counting operations or
types, belongs inline in that test as a raw string instead.

### Fixture tests

`crates/oas3-gen/tests/petstore_*.rs` prove the generated client and server work together. Each
target serves `petstore_server::router` on a free local port, backed by `PetstoreService` in
`tests/support/`, an in-memory implementation of the generated `ApiServer` trait. Tests call it
through `petstore_client::PetstoreClient`. The service records each request it receives, so a
test can assert both what crossed the wire and what came back.

- `#[rstest]` with `#[case]` tables for variants, and the async `support::petstore` fixture
  (`#[future(awt)] petstore: Petstore`) for a fresh store per test.
- `proptest!` for properties over inputs. Cases are synchronous, so a property starts one store
  with `support::start_shared()` and blocks each case on `support::runtime()`.
- Raw `reqwest` requests only where the generated client can't express the input: missing
  credentials, invalid bodies, wire-level `Content-Type` checks.

A test that fails because generated code breaks the spec is a generator bug. Fix the generator
and regenerate; don't weaken the assertion.

A new operation in `petstore.json` adds a method to the generated `ApiServer`, so the build fails
until `PetstoreService` implements it. Give it a `Received` variant and an entry in
`petstore_smoke.rs`, whose coverage check fails while any operation goes unexercised.

## Requirements

- Unit tests in `#[cfg(test)]` modules alongside the code; generator integration tests in
  `src/generator/tests/`; generated-code tests in `tests/`.
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
