# Commands

`cargo run -- generate --help` and `cargo run -- list --help` are the authoritative flag
reference; `ui/cli.rs` is where they're defined. This file covers tooling and its prerequisites.

## Core

```bash
cargo build
cargo test                                    # compiles fixtures/ too
cargo clippy --all -- -W clippy::pedantic     # stricter than CI
cargo +nightly fmt --all                      # nightly required; settings in rustfmt.toml
```

Add `--fix --allow-dirty` to clippy to apply what it can. Per-crate work: append
`-p oas3-gen` or `-p oas3-gen-support`.

CI (`.github/workflows/Lint.yml`) runs `cargo clippy --all` and `cargo deny check` — not
`cargo test`. Test failures surface locally only.

## Generate

```bash
cargo run -- generate types      -i spec.json -o out.rs     # single file
cargo run -- generate client     -i spec.json -o client.rs  # single file
cargo run -- generate client-mod -i spec.json -o out/       # types.rs + client.rs + mod.rs
cargo run -- generate server-mod -i spec.json -o out/       # types.rs + server.rs + mod.rs
cargo run -- list operations     -i spec.json
```

JSON and YAML are auto-detected. Output parent directories are created as needed.

`--workspace` / `-w` (requires `client-mod` or `server-mod`) emits a standalone crate:
`Cargo.toml` plus `src/` with `lib.rs` as module root, package named after the output directory,
dependency versions pinned to what the generator was built against.

Prerequisites for specific flags: `--doc-format` needs `mdformat` on PATH.

## Other tooling

| Command | Needs |
|---|---|
| `cargo deny check` | `cargo-deny` — advisories and licences |
| `cargo tarpaulin --bins --skip-clean -o Markdown` | `cargo-tarpaulin` — coverage |
| `cargo flamegraph -o flamegraph.svg -- generate -i spec.json -o out.rs` | `cargo-flamegraph` |
| `mdbook serve book/` | `mdbook` — preview on :3000 |

`flake.nix` / `.envrc` (`use flake`) provide the toolchain via direnv.

## Book

Feature changes must land in `book/src/`: `client-generation.md` (client calls,
responses, and authentication), `server-generation.md` (service traits, routing,
and credentials), `code-generation.md` (shared types and generation options),
`builders.md` (builder patterns), and `introduction.md` (overview). Update
`SUMMARY.md` when adding or reorganizing chapters. Follow
[book-style-guide.md](book-style-guide.md) for the book's voice and examples.
