# CLAUDE.md

OpenAPI 3.1 -> Rust code generator. Cargo workspace, edition 2024, MSRV 1.89.

- `crates/oas3-gen` — CLI (binary). Pipeline: **parse spec -> convert to AST -> generate Rust**.
- `crates/oas3-gen-support` — runtime library the generated code depends on.

**The source is the source of truth.** These docs map the territory and record decisions that
the code can't state itself. For anything the code already answers — module layout, flag lists,
fragment names, dependency versions — read the code. Never assert generator behavior you
haven't confirmed in the source or in `crates/oas3-gen/fixtures/`.

## Commands

```bash
cargo test                                   # includes compiling fixtures/
cargo clippy --all -- -W clippy::pedantic
cargo +nightly fmt --all                     # nightly required
cargo run -- generate <types|client|client-mod|server-mod> -i spec.json -o out
cargo run -- generate --help                 # authoritative flag list
```

## Non-negotiables

1. Rustdoc (`///`, `//!`) on public API; no inline comments. Naming and structure carry intent.
2. No emojis anywhere.
3. Collection choice determines whether generation is deterministic — see [coding-standards.md](docs/coding-standards.md#collections).
4. Rebuild fixtures after any change to generated output — see [testing.md](docs/testing.md).
5. Document feature changes in `book/src/`.
6. Scope: deliver what was asked and stop. One converter change rewrites thousands of fixture lines.

## Gotchas

- `main.rs` declares `fixtures/` as a `#[cfg(test)]` module, so `cargo test` **compiles** generated
  output. Stale fixtures break the build. `event_stream` is excluded from that module — nothing
  compile-checks it.
- `--doc-format` needs `mdformat` on PATH; its test skips silently when absent.
- CI runs `cargo clippy --all` and `cargo deny check`, not `cargo test`.

## Docs

| Read when | Doc |
|---|---|
| Writing or changing code | [coding-standards.md](docs/coding-standards.md) |
| Deciding where code goes | [architecture.md](docs/architecture.md) |
| Working in `codegen/` | [code-fragments.md](docs/code-fragments.md) |
| Adding tests, rebuilding fixtures | [testing.md](docs/testing.md) |
| Tooling beyond the four commands above | [commands.md](docs/commands.md) |
| Scope, verification, delegation, effort | [agent-guidance.md](docs/agent-guidance.md) |
