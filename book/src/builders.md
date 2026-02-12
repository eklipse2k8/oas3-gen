# Builder Pattern with `bon`

Generated Rust types from OpenAPI schemas tend to have many fields. Some are
required, some are optional, and some have defaults that only matter during
deserialization. When constructing these types by hand in application code,
struct literal syntax can become verbose and error-prone fast.

Consider a `Pet` with four fields:

```rust
let pet = Pet {
    id: 42,
    name: "Whiskers".to_string(),
    tag: None,
    allergies: None,
};
```

That's manageable. Now consider a request struct with path parameters, query
parameters, and headers that all need to be slotted into nested sub-structs:

```rust
let request = ListPetsRequest {
    path: ListPetsRequestPath {
        api_version: "v2".to_string(),
    },
    query: ListPetsRequestQuery {
        limit: Some(25),
    },
    header: ListPetsRequestHeader {
        x_sort_order: None,
        x_only: None,
    },
};
```

That is six lines of ceremony to express two meaningful values. The nested
struct names are long, the optional fields are noise, and the compiler will
not catch a missing field until the developer adds it. As schemas grow, this
pattern scales poorly.

The `--enable-builders` flag solves this by integrating the
[`bon`](https://docs.rs/bon/latest/bon/) crate into the generated code. `bon`
is a compile-time builder generator that uses the typestate pattern to ensure
all required fields are set before construction, with zero runtime cost. It
turns the example above into:

```rust
let request = ListPetsRequest::builder()
    .api_version("v2".to_string())
    .limit(25)
    .build()?;
```

Three lines. No nested structs. No `None` assignments. Required fields are
enforced at compile time, and optional fields can simply be omitted.

---

## Enabling Builders

Pass the `--enable-builders` flag during code generation:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ --enable-builders
```

This works with all generation modes: `types`, `client`, `client-mod`, and
`server-mod`.

## Adding `bon` to Your Project

The generated code references `bon` macros and derives, so the crate must be
present in the consuming project's `Cargo.toml`:

```toml
[dependencies]
bon = "3.8"
```

Without this dependency, the generated code will fail to compile with unresolved
import errors. The `bon` crate is lightweight and has no runtime dependencies
beyond `proc-macro2` and `syn`, which most Rust projects already pull in
transitively.

> **Tip:** If the project already uses `bon` for its own types, there is nothing
> extra to add. The generated code uses the same `bon::Builder` derive and
> `#[builder]` attribute that any hand-written `bon` usage would.

---

## What Changes in the Generated Code

Enabling builders affects two categories of generated types: **schema structs**
and **request structs**. Each gets a different integration point with `bon`.

### Schema Structs

Schema structs (types generated from `components/schemas`) receive a
`bon::Builder` derive. This adds a `::builder()` associated function that
returns a type-safe builder.

**Without `--enable-builders`:**

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub allergies: Option<Box<Health>>,
}
```

**With `--enable-builders`:**

```rust
#[derive(Debug, Clone, PartialEq, Deserialize, bon::Builder)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub allergies: Option<Box<Health>>,
}
```

The struct definition itself is identical except for the extra derive. The
difference shows up at the call site:

```rust
// Struct literal (always available)
let pet = Pet {
    id: 42,
    name: "Whiskers".to_string(),
    tag: Some("indoor".to_string()),
    allergies: None,
};

// Builder (available with --enable-builders)
let pet = Pet::builder()
    .id(42)
    .name("Whiskers".to_string())
    .tag("indoor".to_string())
    .build();
```

Notice that `tag` accepts a plain `String` rather than `Option<String>`. The
builder treats `Option` fields as optional setters: calling `.tag(...)` wraps
the value in `Some` automatically, and omitting the call leaves it as `None`.
The same applies to `allergies`, which can simply be left off when not needed.

### Request Structs

Request structs benefit from builders in a more dramatic way. These types
contain nested sub-structs for path parameters, query parameters, and headers.
Without builders, constructing a request means manually assembling each nested
struct.

When `--enable-builders` is active, the generator produces a `#[builder]`
constructor method that flattens all parameters into a single builder
interface. The nested structs are assembled internally.

**Without `--enable-builders`:**

```rust
pub struct ListPetsRequest {
    pub path: ListPetsRequestPath,
    pub query: ListPetsRequestQuery,
    pub header: ListPetsRequestHeader,
}

// Construction requires knowledge of internal structure
let request = ListPetsRequest {
    path: ListPetsRequestPath {
        api_version: "v2".to_string(),
    },
    query: ListPetsRequestQuery {
        limit: Some(25),
    },
    header: ListPetsRequestHeader {
        x_sort_order: Some(ListCatsRequestHeaderXSortOrder::Asc),
        x_only: None,
    },
};
```

**With `--enable-builders`:**

```rust
pub struct ListPetsRequest {
    pub path: ListPetsRequestPath,
    pub query: ListPetsRequestQuery,
    pub header: ListPetsRequestHeader,
}

#[bon::bon]
impl ListPetsRequest {
    #[builder]
    pub fn new(
        api_version: String,
        limit: Option<i32>,
        x_sort_order: Option<ListCatsRequestHeaderXSortOrder>,
        x_only: Option<Vec<ListPetsRequestHeaderXonly>>,
    ) -> anyhow::Result<Self> {
        let request = Self {
            path: ListPetsRequestPath { api_version },
            query: ListPetsRequestQuery { limit },
            header: ListPetsRequestHeader {
                x_sort_order,
                x_only,
            },
        };
        request.validate()?;
        Ok(request)
    }
}

// Construction is flat and ergonomic
let request = ListPetsRequest::builder()
    .api_version("v2".to_string())
    .limit(25)
    .x_sort_order(ListCatsRequestHeaderXSortOrder::Asc)
    .build()?;
```

Several things are worth noting here:

- **Flat parameter list.** Path, query, and header parameters are all
  promoted to top-level builder setters. There is no need to know which
  sub-struct a parameter belongs to.
- **Optional fields omitted.** `x_only` is not set, so it defaults to `None`.
  No explicit `None` assignment required.
- **Validation included.** The builder's `build()` call runs the same
  `validator::Validate` checks that would normally need to be invoked manually.
  If a required field violates a constraint (for example, a string shorter than
  its minimum length), the builder returns an error.

---

## A Side-by-Side Comparison

To see the full impact, consider a `ShowPetByIdRequest` that takes a path
parameter and a required header:

### Without Builders

```rust
let request = ShowPetByIdRequest {
    path: ShowPetByIdRequestPath {
        pet_id: "pet-123".to_string(),
    },
    header: ShowPetByIdRequestHeader {
        x_api_version: "2024-01-01".to_string(),
    },
};
request.validate()?;
```

The developer must remember to call `.validate()` separately. Forgetting it
means constraints like minimum string length go unchecked at runtime.

### With Builders

```rust
let request = ShowPetByIdRequest::builder()
    .pet_id("pet-123".to_string())
    .x_api_version("2024-01-01".to_string())
    .build()?;
```

Validation is automatic. The required fields `pet_id` and `x_api_version` are
enforced at compile time by the builder's typestate. Calling `.build()` without
setting them is a compilation error.

---

## When Builders Shine

Builders are particularly valuable in a few common scenarios:

**Testing.** Test code often constructs many variations of the same struct.
Builders reduce the noise so the meaningful differences stand out:

```rust
#[test]
fn test_pet_with_tag() {
    let pet = Pet::builder()
        .id(1)
        .name("Buddy".to_string())
        .tag("dog".to_string())
        .build();

    assert_eq!(pet.tag, Some("dog".to_string()));
}

#[test]
fn test_pet_without_tag() {
    let pet = Pet::builder()
        .id(2)
        .name("Mittens".to_string())
        .build();

    assert_eq!(pet.tag, None);
}
```

**Large schemas.** Some OpenAPI specifications define schemas with dozens of
fields. Struct literals for these types become walls of `field: None` lines.
Builders let the developer set only what matters and leave the rest at their
defaults.

**Prototyping and iteration.** When a schema is still evolving, adding a new
optional field does not break existing builder call sites. Struct literals, on
the other hand, require updating every construction site to include the new
field.

---

## Combining with Other Flags

The `--enable-builders` flag composes freely with other code generation options:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ \
    --enable-builders \
    --visibility crate \
    --enum-mode relaxed \
    -c date_time=time::OffsetDateTime
```

Builders respect the chosen visibility level. With `--visibility crate`, the
generated builder methods and derives remain accessible within the crate but
are not part of the public API.

---

## Trade-offs

Every dependency is a trade-off, and `bon` is no exception. Here is what to
consider:

| Consideration | Details |
|---|---|
| **Compile time** | `bon` is a proc-macro crate that adds to compilation. For most projects this is negligible, but very large generated files with hundreds of structs may see a measurable increase. |
| **IDE support** | Builder methods are generated by macros, so some IDEs may not auto-complete them until the project is built once. After that, `rust-analyzer` picks them up normally. |
| **Struct literals still work** | Enabling builders does not remove the ability to construct types with struct literal syntax. Both approaches coexist, and developers can mix them freely. |

For projects that want the lightest possible generated output with zero extra
dependencies, leaving `--enable-builders` off is the right call. For projects
that prioritize developer ergonomics and are already comfortable with proc-macro
dependencies, builders pay for themselves quickly in reduced boilerplate and
fewer construction errors.
