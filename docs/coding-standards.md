# Coding Standards

## CRITICAL: Token Conservation Requirements

- **NO inline comments**: Never add explanatory comments, session notes, or relative-to-session notes within code. Code must be self-documenting through clear naming and structure.
- **NO emojis**: Never use emojis in any context - code, comments, documentation, or messages. Emojis consume valuable tokens.
- **Doc comments only**: Only use proper Rust doc comments (`///` or `//!`) for public API documentation that will be part of generated rustdoc.

This project prioritizes token efficiency. Every inline comment and emoji wastes tokens that could be used for actual code or logic.

## Naming Conventions

Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/naming.html). Prioritize clarity over brevity (prefer `request` over `req`).

### Casing

| Identifier Type                       | Convention           | Examples                                 |
| ------------------------------------- | -------------------- | ---------------------------------------- |
| Crates                                | `kebab-case`         | `oas3-gen`, `oas3-gen-support`           |
| Modules                               | `snake_case`         | `schema_graph`, `operation_registry`     |
| Structs, Enums, Traits, Type Aliases  | `UpperCamelCase`     | `SchemaConverter`, `GenerateMode`        |
| Enum Variants                         | `UpperCamelCase`     | `RequestOnly`, `Bidirectional`           |
| Functions, Methods                    | `snake_case`         | `generate_code`, `convert_schema`        |
| Variables, Fields, Parameters         | `snake_case`         | `spec`, `visibility`, `only_operations`  |
| Constants, Statics                    | `UPPER_SNAKE_CASE`   | `REQUEST_SUFFIX`, `CLIPPY_ALLOWS`        |
| Generic Type Parameters               | `UpperCamelCase`     | `T`, `E`, `IntoSchema`                   |
| Macros                                | `snake_case!`        | `vec!`, `quote!`                         |

### Type Suffixes

- `...Converter`: Type conversion | `...Analyzer`: Data inspection | `...Registry`: Collection storage
- `...Graph`: Graph structures | `...Config`: Configuration | `...Builder`: Builder pattern
- `...Def`: AST nodes (StructDef, EnumDef, TypeAliasDef)

### Function Patterns

- Constructors: `new()`, `with_<property>()`, `from_<source>()`
- Getters: `<property>()` (no `get_` prefix)
- Conversions: `to_<type>()` (non-consuming), `into_<type>()` (consuming)
- Predicates: `is_<condition>()`, `has_<property>()`

### Generated Code

- Distinguish OpenAPI (source) from Rust AST (target) concepts
- Operation types: `...Request`, `...RequestBody`, `...Response`
- Fields: `snake_case` with keyword escaping (`r#type`)

## Collection Types for Deterministic Generation

CRITICAL: Choose collection types carefully to ensure deterministic code generation.

### IndexMap/IndexSet (insertion order)

- `OperationRegistry`: Preserves operation order from OpenAPI spec for logical client method ordering
- Use when spec author's ordering is meaningful and should be reflected in generated code
- Operations should appear in client in same order as spec

### BTreeMap/BTreeSet (sorted order)

- Schema storage, type generation, dependency graphs
- Produces alphabetically sorted output independent of spec ordering
- More stable across spec changes (reordering schemas doesn't change generated output)
- Makes generated code easier to navigate and review
- Example: `deduplicate_and_order_types()` intentionally uses BTreeMap for sorting

### HashMap/HashSet (non-deterministic)

- NEVER use for anything that affects code generation order
- Only acceptable for internal logic where order doesn't matter (e.g., temporary deduplication)

### Rule of thumb

- Operations/endpoints -> IndexMap (spec order matters)
- Types/schemas/dependencies -> BTreeMap (alphabetical is better)
- Internal bookkeeping -> HashMap only if order truly doesn't matter

## Preferred Code Patterns

### Reference Counting and Cloning

- Use `Arc<T>` for shared ownership of expensive-to-clone types (e.g., `Arc<ObjectSchema>`)
- `Arc::clone()` is O(1) and only increments a reference count
- Prefer `Arc` over deep cloning when passing schemas or large data structures through the conversion pipeline
- This reduces memory usage and improves performance

### Vec Initialization

- Prefer `vec![]` over `Vec::new()` for consistency
- Both are idiomatic, but `vec![]` is more concise

### Builder Pattern

- Use builder pattern (via `derive_builder`) for structs with multiple optional fields or complex construction
- Direct struct initialization is acceptable for simple parameter objects with few required fields
- Builders improve readability when constructing objects with many fields
- Example: `FieldDefBuilder::default().name("foo").rust_type(ty).build()?`

### Avoid Tuples

- NEVER use tuples as function return types when returning multiple values
- Use named structs instead for clarity and maintainability
- Good: `fn convert() -> Generated<RustType>` with `struct Generated<T> { item: T, inline_types: Vec<RustType> }`
- Bad: `fn convert() -> (RustType, Vec<RustType>)`
- Tuples lack semantic meaning and make code harder to understand
- Exception: Standard library patterns like `Iterator::enumerate()` where tuple meaning is well-established

### String Enums

- Use `strum` (with `#[derive(EnumString, Display)]`) for simple known string enums
- Provides automatic string parsing and serialization without boilerplate
- Good for enums with fixed string representations like HTTP methods, status categories, etc.
- Example: `#[derive(EnumString, Display)] enum HttpMethod { Get, Post, Put, Delete }`

### String Interning

- Use `string_cache::DefaultAtom` when strings act as symbols (identifiers, type names, field names)
- `DefaultAtom` provides O(1) equality comparison and reduced memory usage through interning
- Wrap strings in `DefaultAtom` using `.into()`: `let name: DefaultAtom = "MyStruct".into()`
- Particularly effective for repeated identifiers in code generation where the same names appear frequently
- Example: Type names, field names, operation IDs, schema references
- Don't use for arbitrary user content or large strings that won't be reused

### Error Context with anyhow

- Use `with_context()` instead of `map_err()` when adding context to errors
- `with_context()` preserves the error chain and is more idiomatic with anyhow
- Bad: `.map_err(|e| anyhow::anyhow!("Failed for '{}': {e}", name))?`
- Good: `.with_context(|| format!("Failed for '{}'", name))?`
- The underlying error is automatically chained; don't manually interpolate it into the message
- Import `use anyhow::Context;` to access the `with_context()` method on `Result` types

### Type-Safe Enums for Configuration

- Use typed enums instead of boolean flags for configuration options
- Makes intent explicit at call sites and enables exhaustive pattern matching
- Good: `enum EnumCasePolicy { Preserve, Deduplicate }` with `config.enum_case == EnumCasePolicy::Preserve`
- Bad: `preserve_case: bool` with `config.preserve_case`
- Example: `CodegenConfig` uses `EnumCasePolicy`, `EnumHelperPolicy`, `EnumDeserializePolicy`, `ODataPolicy`
- Prevents invalid combinations and makes code more self-documenting

### Attribute Types with ToTokens

- Use typed enums for code generation attributes instead of stringly-typed approaches
- Implement `ToTokens` for direct code generation integration
- Good: `enum OuterAttr { SkipSerializingNone, SerdeAs }` implementing `ToTokens`
- Bad: `extra_attrs: Vec<String>` with manual string construction
- Consolidate multiple attributes of the same type into single combined attributes during codegen
- Examples: `OuterAttr`, `SerdeAttribute`, `ValidationAttribute` in `ast/` module

## Design Principles

### SOLID Principles

- Single Responsibility: One concern per module/struct/function
- Open/Closed: Extend via composition, not modification
- Liskov Substitution: Subtypes fully replace base types
- Interface Segregation: Focused traits over monolithic ones
- Dependency Inversion: Depend on abstractions

### Avoid Duplication

- Never duplicate logic; extract to reusable functions/traits/generics
- Search for existing implementations before writing new code
- Refactor duplicated patterns immediately upon discovery

### Code Placement Strategy

1. Review pipeline architecture: Parse/Analyze -> Convert (AST) -> Generate (Rust source)
2. Identify stage:
   - analyzer/ for schema analysis, validation, and type usage tracking
   - naming/ for identifier generation and type name inference
   - converter/ for OpenAPI to AST transformation
   - codegen/ for AST to Rust source code generation
3. Locate module: enums, structs, operations, type_resolver, attributes, cache, etc.
4. Check utilities for cross-cutting concerns: utils/text.rs, naming/identifiers.rs
