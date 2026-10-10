# Testing

`cargo test` runs the whole workspace. Don't use `--lib`: no crate keeps tests in its sources.

## Layout

Tests and fixtures live outside the published crates, so `cargo package` ships neither.

| Path | Holds |
|---|---|
| `fixtures/petstore.json` | The fixture spec, generating the two crates below |
| `fixtures/petstore-client`, `fixtures/petstore-server` | Generated workspace members |
| `fixtures/specs/<area>/` | Minimal specs for `tests/oas3-gen/`, loaded with `spec!("<area>/<file>.json")` |
| `tests/` | `oas3-gen-tests`, a test-only package (`publish = false`) with three targets |
| `tests/oas3-gen/` | Generator tests; the tree mirrors `crates/oas3-gen/src/` |
| `tests/petstore-client/` | The generated client, and the `oas3-gen-support` runtime in `runtime/` |
| `tests/petstore-server/` | The generated server over raw HTTP, and the shared harness in `support/` |
| `benches/` | criterion benches (`generate`, `sse`) declared in `tests/Cargo.toml` |

`oas3-gen` has a library target only so these tests can reach generator internals.

```bash
cargo test -p oas3-gen-tests --test oas3-gen             # one target
cargo test --workspace -- --list | grep -c ': test$'     # suite size
cargo bench -p oas3-gen-tests
```

## Fixtures

One document, `fixtures/petstore.json`, is the fixture: an OpenAPI 3.1.2 pet
store that exercises every feature the generator supports. It generates two crates with
`--workspace`, `fixtures/petstore-client` and `fixtures/petstore-server`. Both are workspace
members, so `cargo build` compiles generated output against its own generated `Cargo.toml`. Any
change to generated code means regenerating, or the build breaks:

```bash
F=fixtures
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
exercise it. A spec that feeds only `tests/oas3-gen/` belongs in `fixtures/specs/<area>/` as a
minimal file. Reuse an existing file before adding one, and never inline a spec as a string.

### Fixture tests

`tests/petstore-client/` and `tests/petstore-server/` are the source of truth that the generated
code implements OpenAPI 3.1.2. Both serve `petstore_server::router` on a free local port, backed
by `PetstoreService` in `tests/petstore-server/support/`, an in-memory implementation of the
generated `ApiServer` trait. The client target includes that harness with `#[path]`. The service
records each request it receives, so a test can assert both what crossed the wire and what came
back.

- `petstore-client` drives `petstore_client::PetstoreClient`. `Wiretap` records the raw requests
  the router receives, `received!` reads the request the service last got, and `answered!`
  matches a response variant.
- `petstore-server` sends raw `reqwest` requests, for input the generated client can't express:
  missing credentials, undecodable bodies, wire-level `Content-Type` checks.
- The async `support::petstore` fixture (`#[future(awt)] petstore: Petstore`) gives each test a
  fresh store. `proptest!` cases are synchronous, so a property starts one store with
  `support::start_shared()` and blocks each case on `support::runtime()`.

A test that fails because generated code breaks the spec is a generator bug. Fix the generator
and regenerate; don't weaken the assertion.

A new operation in `petstore.json` adds a method to the generated `ApiServer`, so the build fails
until `PetstoreService` implements it. Give it a `Received` variant and an entry in `OPERATIONS`
in `tests/petstore-client/smoke.rs`, whose coverage check fails while any operation goes
unexercised.

## What earns a test

The suite stays near 200 entries in `cargo test --workspace -- --list`, where each `#[case]`
counts as one. A new test has to earn its place.

- Fixture tests prove spec conformance. `tests/oas3-gen/` tests exist for refactoring safety:
  they pin observable behavior (parameter and output order, determinism, naming and collisions,
  type-resolution decisions, user-facing errors) through the most stable public seam, so they
  fail when behavior changes, not when internals move.
- No test for what the fixture output already shows, for a constructor or getter, or for a
  dependency's behavior. Add a case to an existing parametrized test before adding a function.
- Never hard-code a value or special-case an assertion to make a test pass. If a test or
  requirement looks wrong, say so.
- `cargo test` plus the fixture diff is the verification step. Nothing to add on top.

## Style

- Every test is `#[rstest]`. Name variants with `#[case::what_it_shows(..)]`, or iterate a case
  table inside one test when the cases are many and cheap. Async tests add `#[tokio::test]`.
- `proptest!` for properties over generated inputs: round trips, chunking, permutation
  invariance. Keep case counts modest so the suite stays fast.
- Fallible tests return `anyhow::Result<()>` and use `?`. Asserting helpers are
  `#[track_caller]`. `#[expect(lint, reason = "..")]` over `#[allow]`.
- `macro_rules!` for repeated shapes: `spec!` (in `tests/oas3-gen/main.rs`), `config!`
  (converter), `generated!` (codegen), `answered!` and `received!` (client).
- Area helpers live in that area's `mod.rs`; helpers shared across areas live in the target's
  `main.rs`.
- No comments of any kind in test code.
- Timing belongs in criterion benches in `benches/`, never in assertions.

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
cargo tarpaulin --workspace --skip-clean -o Markdown   # writes tarpaulin-report.md; delete when done
```
