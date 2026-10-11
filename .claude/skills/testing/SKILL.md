---
name: testing
description: This skill should be used when adding, changing, or running oas3-gen tests or benchmarks, including when the user asks to "add a test", "write a test for this", "run the tests", "rebuild the fixtures", "regenerate the fixtures", "update petstore.json", "add a benchmark", "prove this works", or "check how the generator handles this spec", and whenever a change touches `tests/`, `benches/`, `fixtures/`, or generated output. Covers the two petstore fixtures and the script that builds them, rstest, proptest, and criterion conventions, what earns a test, and scratch crates in /tmp for proofs that don't belong in the repo.
---

# Testing oas3-gen

Tests live only in `oas3-gen-tests`, a test-only package (`publish = false`)
under `tests/`. No crate keeps tests in its sources, so `cargo test --lib` runs
nothing. `oas3-gen` has a library target only so these tests can reach
generator internals.

```bash
cargo test --no-fail-fast                                # whole workspace
cargo test -p oas3-gen-tests --test oas3-gen             # one target
cargo test --workspace -- --list | grep -c ': test$'     # suite size
cargo bench -p oas3-gen-tests                            # criterion benches
```

`--no-fail-fast` matters: a plain `cargo test` stops at the first failing test
binary, and the targets after it never run.

## Test or Proof

Every check is one of two kinds.

- **A test** guards behavior that a future change could break. It lives in
  `tests/` and stays.
- **A proof** answers a question now: what the generator emits for a spec,
  whether an edge case round-trips, whether a fix works. It lives in a scratch
  crate under `/tmp` and is deleted once answered.

Add a test only when a new feature or fix could break under a later change and
no existing test or fixture would catch it. Everything else is a proof. Never
put a proof in `tests/`, `fixtures/specs/`, or anywhere else in the repo, even
temporarily.

### What Earns a Test

The suite stays near 200 entries in `cargo test --workspace -- --list`, where
each `#[case]` counts as one. A new test has to earn its place.

- Fixture tests (`tests/petstore-client/`, `tests/petstore-server/`) prove the
  generated code conforms to OpenAPI 3.1.2.
- `tests/oas3-gen/` tests exist for refactoring safety. They pin observable
  behavior through the most stable public seam, so they fail when behavior
  changes, not when internals move: parameter and output order, determinism,
  naming and collisions, type-resolution decisions, user-facing errors.
- Add a case to an existing parametrized test before adding a function.

### What Doesn't

- Behavior the fixture output already shows.
- A constructor or getter.
- A dependency's behavior.
- Timing. That belongs in a criterion bench.
- A one-off question. Answer it with a scratch proof.

Never hard-code a value or special-case an assertion to make a test pass. If a
test or requirement looks wrong, say so. A fixture test that fails because
generated code breaks the spec is a generator bug: fix the generator and
rebuild the fixtures; don't weaken the assertion.

## Scratch Proofs

`scripts/scratch.sh` builds a throwaway crate at
`/tmp/oas3-gen-scratch/<name>`:

```bash
dir=$(.claude/skills/testing/scripts/scratch.sh <name> [client-mod|server-mod] [spec])
```

- The crate's library is the code generated from `spec` (default
  `fixtures/petstore.json`) in the given mode (default `client-mod`), with the
  fixture flags.
- `[patch.crates-io]` points `oas3-gen-support` at the local crate. Without it,
  generated code builds against the published runtime instead of the one under
  test.
- `tests/` can use `oas3-gen`, `rstest`, `proptest`, `tokio`, `serde_json`, and
  `anyhow`, at the workspace's versions.
- All scratch crates share `/tmp/oas3-gen-scratch/target`. The first build
  takes about 15 seconds; later crates reuse it and take a few.

The script deletes and recreates `<name>`, so keep a minimal spec beside it,
not inside it: `/tmp/oas3-gen-scratch/<name>.json`. Write the proof in
`$dir/tests/scratch.rs`, importing generated types from the crate (a `<name>`
of `pet-roundtrip` is `pet_roundtrip::Pet`) and internals from
`oas3_gen::generator`. Run cargo from inside the crate so its
`.cargo/config.toml` applies:

```bash
cd "$dir" && cargo test
```

When the question is only what the generator emits, skip the crate and read
the output. `--all-schemas` keeps schemas that no operation references, which
a minimal spec usually needs:

```bash
cargo run -- generate types -i /tmp/oas3-gen-scratch/<name>.json -o /tmp/oas3-gen-scratch/<name>.rs --all-schemas
```

Delete `/tmp/oas3-gen-scratch/<name>` when the proof is done. If the proof
shows behavior a future change could break, port it into `tests/` under the
rules above.

## The Fixtures

`fixtures/petstore.json` is the fixture: an OpenAPI 3.1.2 pet store that
exercises every feature the generator supports. It generates two crates with
`--workspace`:

- `fixtures/petstore-client`: the generated client. `petstore_client::PetstoreClient`
  sends each operation and parses its response into `ApiResponse`.
- `fixtures/petstore-server`: the generated server. `petstore_server::ApiServer`
  is the trait a service implements, and `petstore_server::router` serves it
  through axum.

Both are workspace members, so every `cargo build` compiles generated output
against its own generated `Cargo.toml`, with the root `[patch.crates-io]`
supplying the local `oas3-gen-support`. Their files are generated: never edit
them by hand. Any change to generated output, from the generator or from
`petstore.json`, means rebuilding them, or the build breaks:

```bash
.claude/skills/testing/scripts/build-fixtures.sh
```

The script regenerates both crates with `--workspace --enable-builders
--all-schemas --all-headers`, adds `publish = false` to each generated
`Cargo.toml` so `cargo publish --workspace` skips them, then runs
`cargo clippy --fix` with `clippy::pedantic` and `cargo +nightly fmt`. Committed
fixtures are post-clippy and post-rustfmt; without those two steps the diff
fills with lint and formatting noise. It ends by printing
`git diff --stat -- fixtures/`.

The fixture diff is how generated-output changes get reviewed. Read it before
claiming a change is correct.

### Where Specs Go

- A feature the generator gains belongs in `petstore.json`, so the generated
  client and server exercise it.
- A spec that feeds only `tests/oas3-gen/` belongs in `fixtures/specs/<area>/`
  as a minimal file, loaded with `spec!("<area>/<file>.json")`. Reuse an
  existing file before adding one.
- Never inline a spec as a string.

A new operation in `petstore.json` adds a method to the generated `ApiServer`,
so the build fails until `PetstoreService` implements it. Give it a `Received`
variant and an entry in `OPERATIONS` in `tests/petstore-client/smoke.rs`, whose
coverage check fails while any operation goes unexercised.

### Fixture Tests

`tests/petstore-client/` and `tests/petstore-server/` are the source of truth
that the generated code implements OpenAPI 3.1.2. Both serve
`petstore_server::router` on a free local port, backed by `PetstoreService` in
`tests/petstore-server/support/`, an in-memory implementation of the generated
`ApiServer` trait. The client target includes that harness with `#[path]`. The
service records each request it receives, so a test can assert both what
crossed the wire and what came back.

- `petstore-client` drives `PetstoreClient`. `Wiretap` records the raw requests
  the router receives, `received!` reads the request the service last got, and
  `answered!` matches a response variant.
- `petstore-server` sends raw `reqwest` requests, for input the generated client
  can't express: missing credentials, undecodable bodies, wire-level
  `Content-Type` checks.

## Layout

Tests and fixtures live outside the published crates, so `cargo package` ships
neither.

| Path | Holds |
|---|---|
| `fixtures/petstore.json` | The fixture spec, generating the two crates below |
| `fixtures/petstore-client`, `fixtures/petstore-server` | Generated workspace members |
| `fixtures/specs/<area>/` | Minimal specs for `tests/oas3-gen/` |
| `tests/oas3-gen/` | Generator tests; the tree mirrors `crates/oas3-gen/src/` |
| `tests/petstore-client/` | The generated client, and the `oas3-gen-support` runtime in `runtime/` |
| `tests/petstore-server/` | The generated server over raw HTTP, and the shared harness in `support/` |
| `benches/` | criterion benches `generate` and `sse`, declared in `tests/Cargo.toml` |

## Tools

- **rstest.** Every test is `#[rstest]`. Name variants with
  `#[case::what_it_shows(..)]`, or iterate a case table inside one test when the
  cases are many and cheap. Async tests add `#[tokio::test]`. The async
  `support::petstore` fixture (`#[future(awt)] petstore: Petstore`) gives each
  test a fresh store.
- **proptest.** `proptest!` covers properties over generated inputs: round
  trips, chunking, permutation invariance. Keep case counts modest so the suite
  stays fast. Cases are synchronous, so a property starts one store with
  `support::start_shared()` and runs each async case through
  `support::run_case`.
- **criterion.** Benches live in `benches/` with `harness = false` and run with
  `cargo bench -p oas3-gen-tests`. Timing belongs there, never in an assertion.

## Style

- Fallible tests return `anyhow::Result<()>` and use `?`. Asserting helpers are
  `#[track_caller]`. `#[expect(lint, reason = "..")]` over `#[allow]`.
- `macro_rules!` for repeated shapes: `spec!` (in `tests/oas3-gen/main.rs`),
  `config!` (converter), `generated!` (codegen), `answered!` and `received!`
  (client).
- Area helpers live in that area's `mod.rs`; helpers shared across areas live in
  the target's `main.rs`.
- No comments of any kind in test code.

## Debugging

- Inspect with `tracing` or `dbg!()`, or a scratch proof that exercises the
  hypothesis.
- `RUST_BACKTRACE=1` for traces; `cargo expand` for macro and derive output.
- Symptoms in generated output usually originate one or two pipeline stages
  upstream of where they appear. The stages run in order: parse the spec, build
  the schema graph (`schema_registry.rs`), collect operations
  (`operation_registry.rs`), convert schemas and operations (`converter/`),
  post-process the response enum and usage (`postprocess/`), and emit Rust
  (`codegen/`). `orchestrator.rs` sequences them.
- Find the mechanism before editing. Pick an approach and commit; revisit only
  on contradicting evidence.
- Delete scratch crates and temporary files before finishing.

Coverage, when asked for:

```bash
cargo tarpaulin --workspace --skip-clean -o Markdown   # writes tarpaulin-report.md; delete it afterward
```

## Scripts

- **`scripts/build-fixtures.sh`**: rebuilds both fixture crates and prints the
  fixture diff stat.
- **`scripts/scratch.sh`**: creates a scratch crate in `/tmp/oas3-gen-scratch/`
  for a proof and prints its path.
