# Builder Pattern with `bon`

When you construct a generated type with a struct literal, you supply a value
for every field. This includes writing `None` for optional fields you don't
need. A *builder* lets you set fields through methods and then construct the
value with a call to `build()`.

In this chapter, we'll enable builders and use them to construct schema values
and requests. We'll also look at the difference between checking that a
required field is set and validating the value of that field.

The examples use a small pet API. The `Pet` below has four fields; your generated
types will reflect the fields in your own specification. Code excerpts omit
imports and unrelated attributes. Put construction examples inside a function;
examples that use `?` need a compatible return type, such as [`anyhow::Result<()>`][rustdoc-anyhow-result].

Let's begin with a struct literal:

```rust
let pet = Pet {
    id: 42,
    name: "Whiskers".to_string(),
    tag: None,
    allergies: None,
};
```

We want to supply an ID and a name, leaving the optional fields unset. With
builders enabled, we can write that as:

```rust
let pet = Pet::builder()
    .id(42)
    .name("Whiskers".to_string())
    .build();
```

The builder supplies `None` for `tag` and `allergies`. It also requires us to
set `id` and `name` before we can call `build()`. Let's look at how to enable
that API and what the generator adds.

## Enabling Builders

Pass `--enable-builders` during code generation:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ --enable-builders
```

You can use this flag with any generation mode: `types`, `client`, `client-mod`,
or `server-mod`. The generated code uses the
[`bon`](https://docs.rs/bon/3.10.1/bon/) crate to create the builder methods.

## Adding `bon` to Your Project

If you're including generated source files in an existing crate, add `bon` to
that crate's `Cargo.toml`:

```toml
[dependencies]
bon = { version = "3.10", features = ["implied-bounds"] }
```

The compiler needs this dependency to resolve the generated `bon` attributes.
If your crate already declares a compatible version, use that dependency.
When you generate a crate with `--workspace`, the generator adds the dependency
for you. See [Workspace Crate Output](./code-generation.md#workspace-crate-output).

## What Changes in the Generated Code

The generator adds builders in two places. Schema structs receive a
[`bon::Builder`][rustdoc-bon-builder] derive. Request structs receive a constructor with a `#[builder]`
attribute. These produce similar method chains, but their `build()` methods
have different return types.

### Schema Structs

Schema structs represent objects from your OpenAPI schemas. Without builders,
our `Pet` definition looks like this:

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub allergies: Option<Box<Health>>,
}
```

With `--enable-builders`, the generator adds [`bon::Builder`][rustdoc-bon-builder] to the derives:

```rust
#[derive(Debug, Clone, PartialEq, Deserialize, bon::Builder)]
pub struct Pet {
    pub id: i64,
    pub name: String,
    pub tag: Option<String>,
    pub allergies: Option<Box<Health>>,
}
```

The derive adds the `Pet::builder()` associated function. We can use it to set
an optional tag as well as the required fields:

```rust
let pet = Pet::builder()
    .id(42)
    .name("Whiskers".to_string())
    .tag("indoor".to_string())
    .build();
```

Notice that `tag()` accepts a [`String`][rustdoc-string]. The builder wraps it in `Some` for us.
If we leave out that call, `tag` is `None`, as in our first example. The
`allergies` field holds a [`Box<Health>`][rustdoc-box] when set; here, it is `None` because we
haven't set it.

Here, `build()` returns a `Pet` directly. The derive does not call
[`validator::Validate::validate()`][rustdoc-validate-method]. If a schema type has generated validation
and you need to check its constraints, call `validate()` on the constructed
value with the [`validator::Validate`][rustdoc-validate] trait in scope.

### Request Structs

Request structs group parameters into nested structs for paths, query strings,
and headers. For example, the petstore fixture's `ShowPetByIdRequest` contains
both path parameters and headers:

```rust
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    pub header: ShowPetByIdRequestHeader,
}
```

With builders enabled, the generator adds a constructor that accepts the
individual parameters and assembles those nested structs. This excerpt shows
the constructor; the generated request also derives [`validator::Validate`][rustdoc-validate] and
marks the nested fields for validation:

```rust
#[bon::bon]
impl ShowPetByIdRequest {
    /// Create a new request with the given parameters.
    #[builder]
    pub fn new(pet_id: String, x_api_version: String) -> anyhow::Result<Self> {
        let request = Self {
            path: ShowPetByIdRequestPath { pet_id },
            header: ShowPetByIdRequestHeader { x_api_version },
        };
        request.validate()?;
        Ok(request)
    }
}
```

The [`#[bon::bon]`][rustdoc-bon-attr] macro processes the constructor's `#[builder]` attribute,
creating setters for `pet_id` and `x_api_version`.
Calling `build()` calls this constructor, including `request.validate()`.
That's why a request builder returns a [`Result`][rustdoc-result] and the schema builder above
returns its value directly.

The same approach applies to requests with query parameters or optional
headers: their constructor parameters become builder setters. For an
[`Option<T>`][rustdoc-option] parameter, you can supply a `T` or omit the setter to use `None`.

Server requests can also contain credentials for API key or bearer token
authentication. Their builders accept a complete credentials struct through the
`credentials()` setter. Building the request doesn't check whether a credential
is authorized; your service performs that check. See
[Constructing Requests in Tests](./server-generation.md#constructing-requests-in-tests)
for an example and how it differs from HTTP extraction.

### Setter Names

A setter usually has its field's or parameter's name. Two kinds of names can't
work that way, so their setters get a different name:

| Field or parameter | Setter | Reason |
|---|---|---|
| `build` or `builder` | `build_value()`, `builder_value()` | `bon` uses these names itself |
| A name that starts with a digit, such as `_2fa` | `value_2fa()` | `bon` drops a leading underscore, and `2fa` isn't a valid name |

For example, a schema with a `2fa` property generates the field `_2fa`, and you
set it with `.value_2fa(...)`. The field itself keeps its name; only the setter
changes. See [Generated Names](./code-generation.md#generated-names) for how
property names become fields.

## A Side-by-Side Comparison

Let's construct the request both ways. The following examples use the
`ShowPetByIdRequest` from the previous section.

### Without Builders

A struct literal requires us to assemble the path and header values ourselves:

```rust
use validator::Validate;

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

The call to `validate()` checks the constraints after construction. In this
example, both strings must be nonempty. Constructing the struct alone doesn't
perform that check. The generated client also validates a request before
sending it.

### With Builders

The builder accepts the same two values:

```rust
let request = ShowPetByIdRequest::builder()
    .pet_id("pet-123".to_string())
    .x_api_version("2024-01-01".to_string())
    .build()?;
```

This call constructs and validates the request. There are two checks to keep
in mind. If you omit `pet_id()` or `x_api_version()`, you can't call `build()`;
that is a compilation error. If you supply an empty string, the code compiles,
but `build()` returns a validation error at runtime.

`bon` tracks which required setters you've called in the builder's type. This
is called the *typestate pattern*. It checks that required values are present;
the constructor's validation checks the supported constraints on those values.

## Using Builders in Tests

Builders can help when a test only needs to vary a few fields. These tests use
the four-field `Pet` from the start of the chapter:

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

The first test sets a tag, and the second leaves it unset. Neither test needs
to assign `None` to `allergies`. For types with many optional fields, this lets
you keep the setup focused on the values the test uses.

The same property helps when a schema changes. Adding an optional field
usually lets you keep existing builder calls, while struct literals that list
every field need an additional assignment.

## Combining with Other Flags

You can enable builders alongside other generation options. For example, to
keep generated items within your crate and accept enum values regardless of
ASCII letter case, run:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ \
    --enable-builders \
    --visibility crate \
    --enum-mode relaxed
```

The visibility setting applies to the generated structs and constructor
methods. With `--visibility crate`, you can use them within your crate. See
[Visibility](./code-generation.md#visibility) and
[Enum Mode](./code-generation.md#enum-mode) for the other options.

## Trade-offs

Enabling builders adds a procedural macro dependency to your project. `bon`
expands the builder API during compilation, so include that work when assessing
your project's build time. The amount of generated code depends on your schemas
and operations.

You can still use struct literals when builders are enabled. Choose a builder
when setting fields by name or constructing nested requests helps your code,
and use a literal when you want to show the whole value in one place. If you
don't need builders, leave `--enable-builders` off to avoid the `bon` dependency.

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-attr]: https://docs.rs/bon/3.10.1/bon/attr.bon.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.1/bon/derive.Builder.html
[rustdoc-box]: https://doc.rust-lang.org/std/boxed/struct.Box.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-string]: https://doc.rust-lang.org/std/string/struct.String.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-validate-method]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html#tymethod.validate
