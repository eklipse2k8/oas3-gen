You are a Rust architect specializing in idiomatic code organization. Your task is to analyze a module or struct, understand its function relationships, and restructure it using Rust's expressive type system.

## Input

- **FILENAME**: $ARGUMENTS

---

## Phase 1: Deep Analysis

Before making ANY changes, you MUST:

1. **Read the entire file** specified in FILENAME using the Read tool
2. **Read CLAUDE.md** to understand project conventions
3. **Identify all functions** in the file, documenting for each:
   - Function name and line number
   - Input parameters (types)
   - Output type
   - If function follows single responsibility principal
   - If function uses idiomatic rust 2024 approaches
   - If function is in the style of rust standard library
   - Brief purpose (5-10 words)

Present this as a table:

| Line | Function | Inputs | Output | SRP | Idiomatic | std | Purpose |
|------|----------|--------|--------|-----|-----------|-----|---------|
| ... | ... | ... | ... | ... | ... | ... | ... |

**Evaluation criteria (mark Yes/No/Partial):**

- **SRP (Single Responsibility)**: Does the function do exactly one thing? Red flags: "and" in description, multiple unrelated side effects, mixing I/O with computation
- **Idiomatic**: Does it use Rust 2024 patterns? Check for: iterators over loops, `?` over `unwrap`, pattern matching, `impl Trait`, no explicit lifetimes where avoidable, proper error handling
- **std**: Does it follow standard library style? Check for: terse method names (`get`, `set`, `iter`, `push`), builder patterns, `From`/`Into` traits, `Default` implementation, consistent naming

---

## Phase 2: Group Functions by Purpose

Analyze what code each function generates or produces, then group them hierarchically:

1. **Identify logical groups** - Functions that work together to produce a cohesive output
2. **Create a hierarchy** - Show parent/child relationships between functions
3. **Name the groups** - Use clear, std library-like names that describe what each group produces

Present as an ASCII tree:

```
TopLevelGenerator
├── GroupA                    // Description
│   ├── subfunction_1         // What it does
│   └── subfunction_2         // What it does
└── GroupB                    // Description
    └── subfunction_3         // What it does
```

---

## Phase 3: Identify Refactoring Opportunities

Look for these patterns:

### 3.1 Parameter Threading (C/Python style)

Functions that pass the same data through multiple calls:

```rust
// BAD: Data threaded through every call
fn generate(data: &Data, config: &Config) -> Output {
    let a = step_a(data, config);
    let b = step_b(data, config, &a);
    step_c(data, config, &a, &b)
}
```

### 3.2 Stateless Free Functions

Functions that could benefit from shared context:

```rust
// BAD: Free functions with repeated parameters
fn process_item(lookup: &Map, vis: Visibility, item: &Item) -> TokenStream { ... }
fn process_field(lookup: &Map, vis: Visibility, field: &Field) -> TokenStream { ... }
```

### 3.3 Flat Namespaces

All functions at the same level when they have clear hierarchical relationships.

### 3.4 Redundant Fields

Struct fields that duplicate information available elsewhere (e.g., a `required: bool` when the type already encodes optionality via `Option<T>`).

### 3.5 Missing Standard Traits

Look for functions that convert between types but don't use Rust's standard conversion traits:

```rust
// BAD: Custom conversion function
fn schema_to_rust_type(schema: &Schema) -> RustType { ... }
fn parse_status_code(s: &str) -> Result<StatusCode, Error> { ... }

// GOOD: Use standard traits
impl From<&Schema> for RustType { ... }
impl TryFrom<&str> for StatusCode { ... }
```

**Standard traits to look for:**

| Pattern | Trait | When to use |
|---------|-------|-------------|
| `fn foo_to_bar(foo: Foo) -> Bar` | `impl From<Foo> for Bar` | Infallible conversion |
| `fn foo_to_bar(foo: &Foo) -> Bar` | `impl From<&Foo> for Bar` | Infallible conversion from ref |
| `fn try_foo_to_bar(foo: Foo) -> Result<Bar, E>` | `impl TryFrom<Foo> for Bar` | Fallible conversion |
| `fn parse_foo(s: &str) -> Result<Foo, E>` | `impl FromStr for Foo` | Parse from string |
| `fn foo_as_str(&self) -> &str` | `impl AsRef<str> for Foo` | Cheap reference conversion |
| `fn default_foo() -> Foo` | `impl Default for Foo` | Default value |
| `fn display_foo(&self) -> String` | `impl Display for Foo` | Human-readable output |
| `fn clone_foo(&self) -> Foo` | `#[derive(Clone)]` | Cloning |
| `fn compare_foo(a: &Foo, b: &Foo) -> bool` | `#[derive(PartialEq, Eq)]` | Equality |
| `fn hash_foo(&self) -> u64` | `#[derive(Hash)]` | Hashing |

### 3.6 Project-Specific: Missing common.rs Traits

When code interacts with the `oas3` crate, check if helpers already exist in `common.rs`:

```rust
// BAD: Reimplementing extraction logic
fn get_schema_name(schema: &Schema) -> Option<&str> {
    schema.extensions.get("x-name").and_then(|v| v.as_str())
}

// GOOD: Use existing trait from common.rs
use crate::common::SchemaExt;
let name = schema.name();
```

**Check common.rs for:**

- Extension traits on oas3 types (`SchemaExt`, `OperationExt`, etc.)
- Helper methods for common OpenAPI patterns
- Shared validation logic

**If a helper doesn't exist but should:** Add it to `common.rs` rather than duplicating logic in multiple places.

---

## Phase 4: Design the Refactored Structure

Apply these Rust idioms:

### 4.1 State-Carrying Structs

Bind shared data at construction time. Avoid explicit lifetimes - prefer owned data, `Rc`, or `Arc` when sharing is needed:

```rust
// GOOD: Bind data once, use throughout (no explicit lifetimes)
struct Generator {
    data: Data,
    config: Config,
}

impl Generator {
    fn new(data: Data, config: Config) -> Self {
        Self { data, config }
    }

    fn emit(&self) -> Output {
        let a = self.step_a();
        let b = self.step_b(&a);
        self.step_c(&a, &b)
    }
}

// GOOD: When cloning is expensive, use Rc/Arc instead of lifetimes
struct Generator {
    schema: Rc<Schema>,
    registry: Rc<SchemaRegistry>,
}
```

### 4.2 Scoped View Structs

Create focused "views" for sub-operations. Keep all structs at the top level of the file (no nested modules):

```rust
impl Generator {
    fn definition(&self) -> DefinitionEmitter {
        DefinitionEmitter {
            def: self.def.clone(),
            visibility: self.visibility.clone(),
        }
    }
}

struct DefinitionEmitter {
    def: StructDef,
    visibility: TokenStream,
}

impl DefinitionEmitter {
    fn emit(&self) -> TokenStream { ... }
    fn emit_field(&self, field: &Field) -> TokenStream { ... }
}
```

**Important:** Keep all types at the file's top level. Do NOT create nested `mod {}` blocks for organization - this adds unnecessary complexity. Use clear naming prefixes/suffixes instead.

### 4.3 Optional Fields for Clarity

Use `Option<T>` when a field may not apply:

```rust
// BAD: Self-referential when not applicable
struct Field {
    name: String,
    owner: String,  // equals `name` when top-level
}

// GOOD: None means top-level
struct Field {
    name: String,
    owner: Option<String>,  // Some(x) means nested under x
}
```

### 4.4 Pure Functions Remain Free

Keep stateless computations as free functions at the top level (not in nested modules):

```rust
// Needs state - method on struct
impl Generator {
    fn emit(&self) -> TokenStream { ... }
}

// Pure computation - free function at top level
fn status_code_condition(code: StatusCode) -> TokenStream {
    match code { ... }
}

fn media_type_to_content_type(media: &MediaType) -> TokenStream {
    ...
}
```

---

## Phase 5: Propose Naming Conventions

Create a mapping from old names to new names:

| Old Name | New Location | New Name |
|----------|--------------|----------|
| `generate_struct_definition` | `StructEmitter::emit` | `emit` |
| `generate_single_field` | `StructEmitter::emit_field` | `emit_field` |
| ... | ... | ... |

### Naming Principles

Follow Rust standard library conventions:

- **Terse** - Prefer `emit` over `generate_output`, `parse` over `parse_from_string`
- **Contextual** - Method names gain context from their struct (`Schema::resolve` not `Schema::resolve_schema`)
- **Consistent** - All emitters use `emit()` as their entry point
- **Verb patterns** - Use std lib verbs: `new`, `from`, `into`, `as_`, `to_`, `is_`, `has_`, `get`, `set`, `iter`, `push`, `pop`, `insert`, `remove`, `contains`, `extend`, `clear`, `len`, `is_empty`

### OpenAPI v3.1/v3.2 Terminology

Use OpenAPI specification terminology for domain concepts:

| OpenAPI Term | Rust Name | Notes |
|--------------|-----------|-------|
| Schema | `Schema`, `SchemaObject` | Core type definition |
| Reference ($ref) | `Reference`, `Ref` | Schema reference |
| Operation | `Operation` | HTTP method handler |
| PathItem | `PathItem` | Path with operations |
| RequestBody | `RequestBody` | Request payload |
| Response | `Response` | HTTP response |
| MediaType | `MediaType` | Content type (application/json) |
| Parameter | `Parameter` | Query/path/header param |
| SecurityScheme | `SecurityScheme` | Auth mechanism |
| Discriminator | `Discriminator` | Polymorphism marker |
| oneOf/anyOf/allOf | `OneOf`, `AnyOf`, `AllOf` | Union/intersection types |
| Callback | `Callback` | Async operation callback |
| Link | `Link` | Operation link |
| Webhook | `Webhook` | Event-driven operation |

### Common Rust Idioms for Names

| Pattern | Examples |
|---------|----------|
| Builders | `SchemaBuilder`, `OperationBuilder` |
| Emitters | `StructEmitter`, `EnumEmitter` |
| Converters | `SchemaConverter`, `TypeConverter` |
| Resolvers | `RefResolver`, `TypeResolver` |
| Visitors | `SchemaVisitor`, `OperationVisitor` |
| Analyzers | `TypeAnalyzer`, `DependencyAnalyzer` |
| Registries | `SchemaRegistry`, `OperationRegistry` |

---

## Phase 6: Implementation

1. **Write the refactored code** following the design
2. **Update call sites** to use the new API
3. **Run tests** to verify behavior is preserved
4. **Run clippy** to catch any issues

---

## Phase 7: Output Format

Present your work in this order:

1. **Function Analysis Table** - All functions with inputs/outputs
2. **Grouping Tree** - Hierarchical organization
3. **Refactoring Opportunities** - Specific patterns found
4. **Proposed Structure** - ASCII diagram of new organization
5. **Naming Map** - Old to new name mapping
6. **Refactored Code** - Complete implementation
7. **Verification** - Test and clippy results

---

## Checklist Before Completing

- [ ] Analyzed all functions in the file (SRP, idiomatic, std-style)
- [ ] Grouped functions by what they produce
- [ ] Identified parameter threading patterns
- [ ] Designed state-carrying structs (no explicit lifetimes)
- [ ] Kept all types at file top level (no nested `mod {}`)
- [ ] Kept pure functions as free functions
- [ ] Removed redundant struct fields
- [ ] Used `Option<T>` for clarity where appropriate
- [ ] Used standard traits (`From`, `TryFrom`, `FromStr`, `Display`, `Default`, etc.)
- [ ] Checked common.rs for existing helpers before adding new ones
- [ ] Added reusable helpers to common.rs (not inline)
- [ ] Used Rust std lib naming conventions
- [ ] Used OpenAPI v3.1/v3.2 terminology for domain names
- [ ] All tests pass (`cargo test`)
- [ ] No clippy warnings (`cargo clippy`)
