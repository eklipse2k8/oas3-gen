# Code Generation Reference

This document describes all command-line flags that control code generation behavior
in `oas3-gen`. Each flag affects the structure, visibility, or content of generated
Rust code.

## Table of Contents

- [Generation Modes](#generation-modes)
- [Visibility](#visibility)
- [Enum Mode](#enum-mode)
- [Helper Methods](#helper-methods)
- [OData Support](#odata-support)
- [Type Customization](#type-customization)
- [Operation Filtering](#operation-filtering)
- [Schema Filtering](#schema-filtering)

---

## Generation Modes

The positional `mode` argument determines what code is generated.

```text
cargo run -- generate <MODE> -i spec.json -o <OUTPUT>
```

### `types`

Generates type definitions only. Output is a single file.

**Output:** `types.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pet {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "available")]
    Available,
    #[serde(rename = "pending")]
    Pending,
}
```

### `client`

Generates types and HTTP client in a single file. Requires `reqwest`.

**Output:** `client.rs`

```rust
// Types section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pet { /* ... */ }

// Client section
#[derive(Debug, Clone)]
pub struct PetStoreClient {
    pub client: Client,
    pub base_url: Url,
}

impl PetStoreClient {
    pub fn new() -> Self { /* ... */ }

    pub async fn list_pets(&self, request: ListPetsRequest) -> anyhow::Result<ListPetsResponse> {
        /* ... */
    }
}
```

### `client-mod`

Generates a module directory with separate files for types and client.

**Output directory:**

```text
output/
├── mod.rs
├── types.rs
└── client.rs
```

**`mod.rs`:**

```rust
mod types;
mod client;

pub use types::*;
pub use client::*;
```

### `server-mod`

Generates a module directory with types and an Axum server trait.

**Output directory:**

```text
output/
├── mod.rs
├── types.rs
└── server.rs
```

**`server.rs`:**

```rust
pub trait ApiServer: Send + Sync {
    fn list_pets(
        &self,
        request: ListPetsRequest,
    ) -> impl Future<Output = Result<ListPetsResponse, ListPetsError>> + Send;
}

pub fn router<S: ApiServer + 'static>(state: Arc<S>) -> Router {
    Router::new()
        .route("/pets", get(list_pets_handler::<S>))
        .with_state(state)
}
```

---

## Visibility

```text
-C, --visibility <LEVEL>
```

Controls the visibility modifier applied to all generated items.

| Value | Modifier | Use Case |
|-------|----------|----------|
| `public` (default) | `pub` | Library distribution |
| `crate` | `pub(crate)` | Internal crate types |
| `file` | *(none)* | Private implementation |

### Example: `--visibility public`

```rust
pub struct Pet {
    pub id: i64,
    pub name: String,
}

pub enum Status {
    Available,
    Pending,
}

impl Status {
    pub fn available() -> Self { Self::Available }
}
```

### Example: `--visibility crate`

```rust
pub(crate) struct Pet {
    pub(crate) id: i64,
    pub(crate) name: String,
}

pub(crate) enum Status {
    Available,
    Pending,
}

impl Status {
    pub(crate) fn available() -> Self { Self::Available }
}
```

### Example: `--visibility file`

```rust
struct Pet {
    id: i64,
    name: String,
}

enum Status {
    Available,
    Pending,
}

impl Status {
    fn available() -> Self { Self::Available }
}
```

---

## Enum Mode

```text
--enum-mode <MODE>
```

Controls how enum variants with case-only differences are handled.

| Value | Behavior |
|-------|----------|
| `merge` (default) | Merge duplicates; first occurrence is canonical, others become aliases |
| `preserve` | Keep all variants; append numeric suffix to collisions |
| `relaxed` | Merge duplicates; enable case-insensitive deserialization |

### Input Schema

```json
{
  "type": "string",
  "enum": ["ACTIVE", "active", "Active", "PENDING"]
}
```

### Example: `--enum-mode merge`

Variants normalizing to the same identifier are merged. Additional values become
serde aliases.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE", alias = "active", alias = "Active")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}
```

**Deserialization:**

- `"ACTIVE"` → `Status::Active`
- `"active"` → `Status::Active`
- `"Active"` → `Status::Active`
- `"pending"` → Error (case-sensitive)

### Example: `--enum-mode preserve`

Each JSON value becomes a distinct variant. Colliding names receive numeric suffixes.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "active")]
    Active1,
    #[serde(rename = "Active")]
    Active2,
    #[serde(rename = "PENDING")]
    Pending,
}
```

**Deserialization:**

- `"ACTIVE"` → `Status::Active`
- `"active"` → `Status::Active1`
- `"Active"` → `Status::Active2`

### Example: `--enum-mode relaxed`

Generates a custom `Deserialize` implementation that normalizes input to lowercase
before matching.

```rust
#[derive(Debug, Clone, Serialize)]
pub enum Status {
    #[serde(rename = "ACTIVE")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}

impl<'de> serde::Deserialize<'de> for Status {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_ascii_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "pending" => Ok(Self::Pending),
            _ => Err(serde::de::Error::unknown_variant(&s, &["active", "pending"])),
        }
    }
}
```

**Deserialization:**

- `"ACTIVE"` → `Status::Active`
- `"active"` → `Status::Active`
- `"Active"` → `Status::Active`
- `"PENDING"` → `Status::Pending`
- `"pending"` → `Status::Pending`

---

## Helper Methods

```text
--no-helpers
```

Disables generation of ergonomic constructor methods for enum variants.

Helper methods are generated for variants that wrap structs with default
implementations. They allow constructing variants with minimal boilerplate.

### Default (helpers enabled)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
}

impl ContentBlock {
    pub fn text(text: String) -> Self {
        Self::Text(TextBlock {
            text,
            ..Default::default()
        })
    }

    pub fn image(source: Box<ImageSource>) -> Self {
        Self::Image(ImageBlock {
            source,
            ..Default::default()
        })
    }

    pub fn code(code: String) -> Self {
        Self::Code(CodeBlock {
            code,
            ..Default::default()
        })
    }
}
```

**Usage:**

```rust
let block = ContentBlock::text("Hello, world!".to_string());
```

### With `--no-helpers`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
}

// No impl block generated
```

**Usage:**

```rust
let block = ContentBlock::Text(TextBlock {
    text: "Hello, world!".to_string(),
    ..Default::default()
});
```

---

## OData Support

```text
--odata-support
```

Enables OData-specific field optionality rules. Fields starting with `@odata.`
are made optional even when listed in the schema's `required` array.

This accommodates Microsoft Graph and other OData APIs where metadata fields
are declared required but frequently omitted in responses.

### Input Schema

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "@odata.type": { "type": "string" },
    "@odata.id": { "type": "string" }
  },
  "required": ["id", "@odata.type", "@odata.id"]
}
```

### Default (OData support disabled)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "@odata.type")]
    pub odata_type: String,
    #[serde(rename = "@odata.id")]
    pub odata_id: String,
}
```

Deserialization fails if `@odata.type` or `@odata.id` are missing from the response.

### With `--odata-support`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "@odata.type")]
    pub odata_type: Option<String>,
    #[serde(rename = "@odata.id")]
    pub odata_id: Option<String>,
}
```

Deserialization succeeds when OData metadata fields are absent.

**Constraints:** OData optionality only applies when:

- Field name starts with `@odata.`
- Parent schema has no discriminator
- Parent schema is not an intersection type

---

## Type Customization

```text
-c, --customize <TYPE=PATH>
```

Overrides the default type mapping for specific primitive types. Uses `serde_with`
for custom serialization.

| Key | OpenAPI Format | Default Type |
|-----|----------------|--------------|
| `date_time` | `date-time` | `chrono::DateTime<Utc>` |
| `date` | `date` | `chrono::NaiveDate` |
| `time` | `time` | `chrono::NaiveTime` |
| `duration` | `duration` | `std::time::Duration` |
| `uuid` | `uuid` | `uuid::Uuid` |

Multiple customizations can be specified:

```bash
cargo run -- generate types -i spec.json -o types.rs \
  -c date_time=time::OffsetDateTime \
  -c date=time::Date \
  -c uuid=my_crate::CustomUuid
```

### Default (no customization)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub scheduled_date: chrono::NaiveDate,
}
```

### With `-c date_time=time::OffsetDateTime`

```rust
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde_as(as = "time::OffsetDateTime")]
    pub created_at: time::OffsetDateTime,
    pub scheduled_date: chrono::NaiveDate,
}
```

### Handling Optional and Array Fields

Customizations automatically wrap in `Option<>` and `Vec<>` as needed:

```rust
#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    #[serde_as(as = "time::OffsetDateTime")]
    pub start: time::OffsetDateTime,

    #[serde_as(as = "Option<time::OffsetDateTime>")]
    pub end: Option<time::OffsetDateTime>,

    #[serde_as(as = "Vec<time::OffsetDateTime>")]
    pub milestones: Vec<time::OffsetDateTime>,

    #[serde_as(as = "Option<Vec<time::OffsetDateTime>>")]
    pub optional_dates: Option<Vec<time::OffsetDateTime>>,
}
```

---

## Operation Filtering

```text
--only <id1,id2,...>
--exclude <id1,id2,...>
```

Filters which operations are included in generated client or server code.
These flags are mutually exclusive.

### `--only`

Generates code only for the specified operation IDs. All other operations are
excluded.

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --only listPets,createPet
```

**Generated client:**

```rust
impl PetStoreClient {
    pub async fn list_pets(&self, request: ListPetsRequest) -> anyhow::Result<ListPetsResponse> {
        /* ... */
    }

    pub async fn create_pet(&self, request: CreatePetRequest) -> anyhow::Result<CreatePetResponse> {
        /* ... */
    }

    // No other methods generated
}
```

### `--exclude`

Generates code for all operations except the specified IDs.

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --exclude deletePet
```

**Generated client:**

```rust
impl PetStoreClient {
    pub async fn list_pets(&self, ...) -> ... { /* ... */ }
    pub async fn create_pet(&self, ...) -> ... { /* ... */ }
    pub async fn get_pet(&self, ...) -> ... { /* ... */ }
    pub async fn update_pet(&self, ...) -> ... { /* ... */ }
    // delete_pet NOT generated
}
```

### Schema Dependency Resolution

When filtering operations, schemas are automatically included based on
transitive dependencies:

1. Collect all schemas referenced by selected operations (parameters, request
   bodies, responses)
2. Expand to include all schemas those schemas depend on
3. Generate only the resulting set of types

**Example:** If `listPets` returns `Pet[]` and `Pet` contains a `Category` field,
both `Pet` and `Category` are generated even though only `listPets` was selected.

---

## Schema Filtering

```text
--all-schemas
```

By default, only schemas reachable from selected operations are generated. This
flag overrides that behavior to generate all schemas defined in the specification.

### Default (reachability filtering)

Given a spec with schemas `Pet`, `Category`, `Store`, `Inventory` where only
`Pet` and `Category` are used by any operation:

```bash
cargo run -- generate types -i spec.json -o types.rs
```

**Generated:** `Pet`, `Category`
**Skipped:** `Store`, `Inventory` (reported as orphaned schemas)

### With `--all-schemas`

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
```

**Generated:** `Pet`, `Category`, `Store`, `Inventory`

### Combining with Operation Filtering

The `--all-schemas` flag is in the same argument group as `--only` and `--exclude`.
You cannot combine them directly.

To generate all schemas while filtering operations, generate types separately:

```bash
# Generate all types
cargo run -- generate types -i spec.json -o types.rs --all-schemas

# Generate filtered client (will only include operation-referenced types inline)
cargo run -- generate client -i spec.json -o client.rs --only listPets
```

---

## Flag Summary

| Flag | Default | Description |
|------|---------|-------------|
| `mode` | `types` | Generation mode: `types`, `client`, `client-mod`, `server-mod` |
| `-C, --visibility` | `public` | Item visibility: `public`, `crate`, `file` |
| `--enum-mode` | `merge` | Enum duplicate handling: `merge`, `preserve`, `relaxed` |
| `--no-helpers` | `false` | Disable enum constructor helpers |
| `--odata-support` | `false` | Make `@odata.*` fields optional |
| `-c, --customize` | *(none)* | Custom type mapping (repeatable) |
| `--only` | *(none)* | Include only specified operations |
| `--exclude` | *(none)* | Exclude specified operations |
| `--all-schemas` | `false` | Generate all schemas regardless of usage |
