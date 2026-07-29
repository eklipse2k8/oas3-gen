# Architecture

Run `ls crates/oas3-gen/src/generator/` for the layout. This file covers only what the tree
doesn't show: stage order, data-flow rules, and where a change belongs.

## Pipeline

One-way data flow. Each stage consumes the previous stage's output and never writes back.

| # | Stage | Entry point |
|---|---|---|
| 1 | Parse spec (JSON/YAML auto-detected) | `oas3` crate, `utils/spec.rs` |
| 2 | Build dependency graph, cycles, merged schemas, discriminators | `schema_registry.rs` |
| 3 | Collect operations and webhooks | `operation_registry.rs` |
| 4 | Convert schemas -> `Vec<RustType>` | `converter/` (`SchemaConverter`) |
| 5 | Convert operations -> `Vec<OperationInfo>` + types + usage | `converter/operations.rs` |
| 6 | Propagate usage, fix serde modes, dedupe response enums | `postprocess/` |
| 7 | Emit Rust source | `codegen/` (`SchemaCodeGenerator`) |

`orchestrator.rs` sequences all seven. `SharedSchemaCache` (`converter/cache.rs`) dedupes within
stage 4-5 only; it does not feed back to stages 2-3.

## Module responsibilities

| Module | Owns |
|---|---|
| `utils/` | Cross-cutting: `$ref` resolution, spec loading, `SchemaExt` schema predicates |
| `naming/` | Rust identifier generation, collision resolution, variant prefix inference |
| `converter/` | OpenAPI semantics -> AST. Where spec quirks get interpreted |
| `ast/` | AST nodes plus their attribute/derive/doc `ToTokens` impls. `RustType` (the 5-variant dispatch root) is in `ast/mod.rs` |
| `postprocess/` | Whole-program passes over `Vec<RustType>` after conversion |
| `codegen/` | AST -> Rust source via `quote`. See [code-fragments.md](code-fragments.md) |
| `ui/` | clap definitions and command handlers |

## Where does my change go?

1. Identify the stage from the table above; a symptom in generated output usually originates
   one or two stages earlier than where it appears.
2. Interpreting the spec differently -> `converter/`. Changing emitted syntax -> `codegen/`.
   Changing a name -> `naming/`. Needing global knowledge of all types -> `postprocess/`.
3. Check `utils/schema_ext.rs` and `naming/identifiers.rs` before adding a helper — the
   predicate you want usually exists.

## Invariants

- Stages never mutate earlier stages' outputs.
- Spec declaration order survives to generated output; see
  [coding-standards.md](coding-standards.md#collections).
- Generation is a pure function of (spec, `CodegenConfig`). No clocks, no filesystem reads, no
  hash-order iteration.
- `CodegenConfig` (`converter/mod.rs`) carries all CLI-driven policy. Route new options through it
  rather than threading flags. `ConverterContext` bundles it with the registry, cache, and usage
  recorder, shared by `Rc` across stages 4-5.

## Dependencies

Pinned in the root `[workspace.dependencies]`; crates inherit with `dep = { workspace = true }`.
Read `Cargo.toml` for versions. Load-bearing choices: `oas3` (parser), `quote`/`proc-macro2` +
`prettyplease` (emission), `petgraph` (dependency graph), `indexmap` (determinism),
`string_cache` (interned identifiers), `bon` (generated builders), `axum` (generated servers).
