# Builder Pattern with `bon`

When you construct a generated type with a struct literal, you supply a value for every
field. This includes writing `None` for optional fields you don't need. A *builder* lets
you set fields through methods and then construct the value with a call to `build()` .

Schema builders return a value directly. Request builders also validate the request and
return a `Result` . We'll use both forms and see which checks happen during compilation
and which happen at runtime.

The examples use the petstore fixture, `fixtures/petstore.json` . Its `NewPet` schema
holds the fields a caller supplies when adding a pet: a required `name` and `species` ,
and five optional fields. Your generated types will reflect the fields in your own
specification. Code excerpts omit imports. Put construction examples inside a function;
examples that use `?` need a compatible return type, such as
[`anyhow::Result<()>`][rustdoc-anyhow-result] .

Let's begin with a struct literal:

```rust
let pet = NewPet {
    name: "Biscuit".to_string(),
    species: Species::Dog,
    nickname: None,
    birth_date: None,
    weight_kg: None,
    microchip: None,
    tags: None,
};
```

We want to supply a name and a species, leaving the optional fields unset. With builders
enabled, we can write that as:

```rust
let pet = NewPet::builder()
    .name("Biscuit".to_string())
    .species(Species::Dog)
    .build();
```

The builder supplies `None` for the optional fields and requires `name` and `species`
before `build()` is available.

## Enabling Builders

Pass `--enable-builders` during code generation:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ --enable-builders
```

You can use this flag with any generation mode: `types` , `client` , `client-mod` , or
`server-mod` . The generated code uses the [`bon`](https://docs.rs/bon/3.10.2/bon/)
crate to create the builder methods.

## Adding `bon` to Your Project

If you're including generated source files in an existing crate, add `bon` to that
crate's `Cargo.toml` :

```toml
[dependencies]
bon = { version = "3.10", features = ["implied-bounds"] }
```

The compiler needs this dependency to resolve the generated `bon` attributes. If your
crate already declares a compatible version, use that dependency. When you generate a
crate with `--workspace` , the generator adds the dependency for you. See
[Workspace Crate Output](./generation-options.md#workspace-crate-output) .

## What Changes in the Generated Code

The generator adds builders in two places. Schema structs receive a
[`bon::Builder`][rustdoc-bon-builder] derive. Request structs receive a constructor with
a `#[builder]` attribute. These produce similar method chains, but their `build()`
methods have different return types.

### Schema Structs

Schema structs represent objects from your OpenAPI schemas. Without builders, our
`NewPet` definition looks like this:

```rust
/// The fields a caller supplies when adding a pet.
#[serde_with::skip_serializing_none]
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    validator::Validate,
    oas3_gen_support::Default
)]
pub struct NewPet {
    #[validate(length(min = 1u64, max = 64u64))]
    pub name: String,
    /// Kinds of pets the store cares for
    pub species: Species,
    pub nickname: Option<String>,
    #[serde(rename = "birthDate")]
    pub birth_date: Option<chrono::NaiveDate>,
    #[serde(rename = "weightKg")]
    #[validate(range(max = 500.0, exclusive_min = 0.0))]
    pub weight_kg: Option<f64>,
    #[validate(regex(path = "REGEX_NEW_PET_MICROCHIP"))]
    pub microchip: Option<String>,
    #[validate(length(max = 10u64))]
    pub tags: Option<indexmap::IndexSet<String>>,
}
```

With `--enable-builders` , the generator adds [`bon::Builder`][rustdoc-bon-builder] to
the end of the derive list. The rest of the definition doesn't change, so this excerpt
shows only the new `derive` attribute:

```rust
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    validator::Validate,
    oas3_gen_support::Default,
    bon::Builder
)]
```

The derive adds the `NewPet::builder()` associated function. We can use it to set an
optional weight as well as the required fields:

```rust
let pet = NewPet::builder()
    .name("Biscuit".to_string())
    .species(Species::Dog)
    .weight_kg(12.5)
    .build();
```

Notice that `weight_kg()` accepts an [`f64`][rustdoc-f64] . The builder wraps it in
`Some` for us. If we leave out that call, `weight_kg` is `None` , as in our first
example.

Here, `build()` returns a `NewPet` directly. The derive does not call
[`validator::Validate::validate()`][rustdoc-validate-method] . The `#[validate]`
attributes above come from the schema's constraints: `name` must be 1 to 64 characters
long, `weight_kg` must be greater than 0 and at most 500, `microchip` must match the
schema's pattern, and `tags` can hold at most 10 entries. To check them, call
`validate()` on the constructed value with the [`validator::Validate`][rustdoc-validate]
trait in scope:

```rust
use validator::Validate;

pet.validate()?;
```

Validation depends on the schema's constraints. For example, a `multipleOf` constraint
becomes a custom check: the petstore's `CollarTelemetry` schema declares `multipleOf: 5`
for `batteryPercent` , so that field validates with `oas3_gen_support::multiple_of::<5>`
.

### Request Structs

Request structs group parameters into nested structs for paths, query strings, and
headers. For example, the petstore fixture's `ShowPetByIdRequest` contains both a path
parameter and headers. It derives [`validator::Validate`][rustdoc-validate] and marks
the headers for nested validation:

```rust
/// Get a pet
#[derive(Debug, Clone, validator::Validate, oas3_gen_support::Default)]
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    #[validate(nested)]
    pub header: ShowPetByIdRequestHeader,
}
```

With builders enabled, the generator adds a constructor that accepts the individual
parameters and assembles those nested structs:

```rust
#[bon::bon]
impl ShowPetByIdRequest {
    /// Create a new request with the given parameters.
    #[builder]
    pub fn new(
        pet_id: uuid::Uuid,
        if_none_match: Option<String>,
        x_api_version: String,
    ) -> anyhow::Result<Self> {
        let request = Self {
            path: ShowPetByIdRequestPath { pet_id },
            header: ShowPetByIdRequestHeader {
                if_none_match,
                x_api_version,
            },
        };
        request.validate()?;
        Ok(request)
    }
}
```

The [`#[bon::bon]`][rustdoc-bon-attr] macro processes the constructor's `#[builder]`
attribute, creating setters for `pet_id` , `if_none_match` , and `x_api_version` .
Calling `build()` calls this constructor, including `request.validate()` . That's why a
request builder returns a [`Result`][rustdoc-result] and the schema builder above
returns its value directly.

The same approach applies to requests with query parameters or request bodies: their
constructor parameters become builder setters. For an [`Option<T>`][rustdoc-option]
parameter, such as the optional `If-None-Match` header's `if_none_match` , you can
supply a `T` or omit the setter to use `None` .

Server requests can also contain credentials for API key or bearer token authentication.
Their builders accept a complete credentials struct through the `credentials()` setter.
Building the request doesn't check whether a credential is authorized; your service
performs that check. See
[Constructing Requests in Tests](./server-generation.md#constructing-requests-in-tests)
for an example and how it differs from HTTP extraction.

### Setter Names

Setters usually have the same names as their fields or parameters. These names need an
adjustment:

| Field or parameter | Setter | Reason |
|---|---|---|
| `build` or `builder` | `build_value()`, `builder_value()` | `bon` uses these names itself |
| A name that starts with a digit, such as `_2fa` | `value_2fa()` | `bon` drops a leading underscore, and `2fa` isn't a valid name |

For example, a schema with a `2fa` property generates the field `_2fa` , and you set it
with `.value_2fa(...)` . The field itself keeps its name; only the setter changes. See
[Generated Names](./schema-types.md#generated-names) for how property names become
fields.

## A Side-by-Side Comparison

Let's construct the request both ways. The following examples use the
`ShowPetByIdRequest` from the previous section, and a `pet_id` of type
[`uuid::Uuid`][rustdoc-uuid] , such as one parsed with `Uuid::parse_str()` .

### Without Builders

A struct literal requires us to assemble the path and header values ourselves:

```rust
use validator::Validate;

let request = ShowPetByIdRequest {
    path: ShowPetByIdRequestPath { pet_id },
    header: ShowPetByIdRequestHeader {
        if_none_match: None,
        x_api_version: "2026-10".to_string(),
    },
};
request.validate()?;
```

The call to `validate()` checks the constraints after construction. In this example,
`x_api_version` must be nonempty: the generator adds that check to a required string
whose schema doesn't set its own length limits. Constructing the struct alone doesn't
perform the check. The generated client also validates a request before sending it.

### With Builders

The builder needs only the two required values:

```rust
let request = ShowPetByIdRequest::builder()
    .pet_id(pet_id)
    .x_api_version("2026-10".to_string())
    .build()?;
```

This call constructs and validates the request. Omitting `pet_id()` or `x_api_version()`
makes the call to `build()` a compilation error. If you supply an empty string to
`x_api_version()` , the code compiles, but `build()` returns a validation error at
runtime.

`bon` tracks which required setters you've called in the builder's type. This is called
the *typestate pattern*. It checks that required values are present; the constructor's
validation checks the supported constraints on those values.

## Using Builders in Tests

Builders can help when a test only needs to vary a few fields. These tests use the
`NewPet` from the start of the chapter:

```rust
#[test]
fn test_new_pet_with_weight() {
    let pet = NewPet::builder()
        .name("Biscuit".to_string())
        .species(Species::Dog)
        .weight_kg(12.5)
        .build();

    assert_eq!(pet.weight_kg, Some(12.5));
}

#[test]
fn test_new_pet_without_weight() {
    let pet = NewPet::builder()
        .name("Mittens".to_string())
        .species(Species::Cat)
        .build();

    assert_eq!(pet.weight_kg, None);
}
```

The first test sets a weight, and the second leaves it unset. Neither test needs to
assign `None` to the other optional fields. For types with many optional fields, this
lets you keep the setup focused on the values the test uses.

Builders can also reduce changes to calling code when a schema changes. Adding an
optional field usually lets you keep existing builder calls, while struct literals that
list every field need an additional assignment.

## Combining with Other Flags

You can enable builders alongside other generation options. For example, to keep
generated items within your crate and accept enum values regardless of ASCII letter
case, run:

```bash
oas3-gen generate client-mod -i api.json -o src/api/ \
    --enable-builders \
    --visibility crate \
    --enum-mode relaxed
```

The visibility setting applies to the generated structs and constructor methods. With
`--visibility crate` , you can use them within your crate. See
[Visibility](./generation-options.md#visibility) and
[Enum Mode](./enum-types.md#enum-mode) for the other options.

## Trade-offs

Enabling builders adds a procedural macro dependency to your project. `bon` expands the
builder API during compilation, so include that work when assessing your project's build
time. The amount of generated code depends on your schemas and operations.

You can still use struct literals when builders are enabled. Choose a builder when
setting fields by name or constructing nested requests helps your code, and use a
literal when you want to show the whole value in one place. If you don't need builders,
leave `--enable-builders` off to avoid the `bon` dependency.

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-attr]: https://docs.rs/bon/3.10.2/bon/attr.bon.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.2/bon/derive.Builder.html
[rustdoc-f64]: https://doc.rust-lang.org/std/primitive.f64.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-result]: https://doc.rust-lang.org/std/result/enum.Result.html
[rustdoc-uuid]: https://docs.rs/uuid/1.27.0/uuid/struct.Uuid.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-validate-method]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html#tymethod.validate
