# Coding Standards

Normative rules. Where the reason isn't obvious from the rule, it's stated — those are the ones
that bite. Scope and verification live in [agent-guidance.md](agent-guidance.md).

## Comments

Rustdoc (`///`, `//!`) on public API. No inline comments — the same patterns repeat across
hundreds of fragments, so explanatory comments become review overhead. No emojis anywhere;
generated code lands in users' source trees.

Don't add docs, comments, or type annotations to code you didn't change.

## Scope of a change

- No features, refactors, or improvements beyond the request.
- No error handling or validation for cases that can't occur. Validate at boundaries only: CLI
  input and the parsed spec. Trust internal invariants.
- No helpers or abstractions for one-time operations; no designing for hypothetical spec features.

## Collections

Determinism is the whole point: identical spec in, byte-identical Rust out.

| Type | Use for |
|---|---|
| `IndexMap`/`IndexSet` | Anything whose order reaches generated output — schemas, fields, enum variants, union variants, operations, discriminator mappings, headers |
| `BTreeMap`/`BTreeSet` | Canonical output not derived from spec order — grouped `use` statements, derives, regex lookup tables |
| `HashMap`/`HashSet` | Internal bookkeeping only, where order cannot reach output |

`.into_group_map()` and `.counts()` return `HashMap` — always iterate `.keys().sorted()`.

Generated-code collection types are policy-driven, not hard-coded. `CollectionTypePolicy` on
`CodegenConfig`: `Ordered` (default) emits `indexmap::IndexMap`/`IndexSet`; `Hashed`
(`--no-ordered-collections`) emits `std::collections::HashMap`/`Vec`. New emission points must
route through `config.map_type_path()` / `config.ordered_collections()`. This affects emitted
type names only — the generator's own structures stay on `IndexMap`/`BTreeMap` per the table.

## Iteration

Use `itertools` rather than reimplementing it: `sorted_by_key`, `unique_by`, `dedup`, `kmerge`,
`chunk_by`, `exactly_one`, `at_most_one`. Its sorts are stable, which determinism depends on.
Skip it for plain `map`/`filter`/`flat_map` and where you need laziness (its sorts collect eagerly).

**Never nest an iterator inside an iterator closure.** Building an inner iterator per outer
element is O(n²) and defeats optimization. Build a lookup map first (`into_group_map_by`), then a
single `flat_map` pass.

Return `impl Iterator` from functions whose results get processed further; defer `collect()` to
the final consumer. Never allocate inside a closure that runs per element.

## Naming

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html). Clarity
over brevity (`request`, not `req`). Standard casing: `snake_case` modules/functions/fields,
`UpperCamelCase` types/variants, `UPPER_SNAKE_CASE` constants, `kebab-case` crates.

Type suffixes signal role: `...Converter` (transformation), `...Analyzer` (inspection),
`...Registry` (collection), `...Graph`, `...Config`, `...Builder`, `...Def` (AST node),
`...Fragment` (codegen unit).

Functions: `new`/`with_<prop>`/`from_<source>`; getters without `get_`; `to_<type>` non-consuming
vs `into_<type>` consuming; `is_`/`has_` predicates.

Keep OpenAPI (source) and Rust AST (target) vocabulary distinct in the same scope. Generated
operation types are `...Request`, `...RequestBody`, `...Response`; generated fields are
`snake_case` with keyword escaping (`r#type`).

## Patterns

- `.collect::<Vec<_>>()` turbofish, not a type annotation on the binding.
- `vec![]` over `Vec::new()`. `into_iter()` over `.iter().cloned()`.
- `let-else` for early returns; `bool::then` for `Option` construction; early-return over nesting.
- `From`/`Into` over ad-hoc conversion methods. Type aliases to name tuple semantics.
- Trait-for-method-resolution imports (`use quote::ToTokens as _;`) at module level, never inside
  a function.
- `Arc<T>` for shared ownership of expensive clones (schemas); `Arc::clone` is a refcount bump.
- `string_cache::DefaultAtom` for symbol-like strings (type/field/operation names) — O(1) equality
  and interning pay off when identifiers repeat. Not for user content or one-off strings.
- `bon` builders for many-optional-field structs; direct initialization for simple ones.
- `strum` (`EnumString`, `Display`) for fixed string enums.
- Typed enums over boolean flags for config, so call sites read as intent and matches stay
  exhaustive (`EnumCasePolicy`, `ODataPolicy`, `CollectionTypePolicy`, …).
- Typed attribute enums implementing `ToTokens` (`OuterAttr`, `SerdeAttribute`,
  `ValidationAttribute`) over building attribute strings.
- `anyhow::Context::context()` for error context — `with_context()` only when the message needs
  computing. Never interpolate the source error; it chains automatically.
- Extension traits for external types (`SchemaExt` on `oas3::spec::ObjectSchema`) when the
  operation reads as a method on the type.

## Structure

- Keep mutable state inside a struct behind `&mut self`. Avoid `&mut` accumulator/cache
  parameters — they thread through layers and obscure ownership. Free functions stay pure.
- No tuples in public return types; name a struct. Exceptions: std idioms (`enumerate`, `unzip`),
  private helpers with a type alias, immediately-destructured intermediates.
- Don't invent a struct just to hand data from one producer to one consumer. If a generic
  container already models the shape (`ConversionOutput<T>`), use it, and let the consumer derive
  secondary values instead of pre-computing them upstream.
- Prefer several focused registries over one mixed-concern cache (`NameRegistry`,
  `SchemaIdentity`, `EnumRegistry` composed by `SharedSchemaCache`).
- SOLID applies as usual: one concern per unit, extend by composition, focused traits.
