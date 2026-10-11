# Schemas and Types

Schemas describe the values an API accepts and returns. This chapter explains how the
generator names fields, chooses collection types, applies defaults, and validates
constraints. For enum variants and unions, see [Enums and Unions](./enum-types.md) .

The examples use `fixtures/petstore.json` unless stated otherwise. Rust excerpts may
omit imports, attributes, and unrelated fields. Commands run from the repository
checkout; replace `cargo run --` with `oas3-gen` when using an installed executable, and
substitute your document's path for `spec.json` .

- [Generated Names](#generated-names)
- [Ordering and Collections](#ordering-and-collections)
- [Field Defaults](#field-defaults)
- [Constant Properties](#constant-properties)
- [Field Validation](#field-validation)
- [OData Support](#odata-support)
- [Type Customization](#type-customization)

## Generated Names

Schema and property names become Rust identifiers. When a name isn't a valid identifier,
or would collide with something Rust or the generated code already uses, the generator
changes it as shown in the following table:

| Name in the specification | Generated name | Reason |
|---|---|---|
| A property named after a keyword, such as `type` or `match` | `r#type` | Keywords need the raw identifier prefix |
| A property named `crate`, `self`, or `super` | `crate_`, `self_`, `super_` | These keywords can't be raw identifiers |
| A property that starts with a digit, such as `2fa` | `_2fa` | Identifiers can't start with a digit |
| A schema named after a prelude item, such as `Option` or `Vec` | `OptionType` | The name would shadow the prelude |
| A schema named `Self` | `SelfType` | `Self` can't be a raw identifier |
| A schema named `S` | `SType` | Generated server handlers use `S` for your service type |
| A schema that starts with a digit, such as `123Response` | `T123Response` | Identifiers can't start with a digit |

A renamed property keeps its original name on the wire through `#[serde(rename)]` , so
the JSON stays the same. Enum variants follow the same rules as schema names. For
example, the enum value `self` becomes the variant `SelfType` .

## Ordering and Collections

By default, the generator preserves declaration order from the OpenAPI document for
schemas, fields, variants, operations, and header constants. You can change enum
ordering separately with [`--enum-layout`](./enum-types.md#enum-layout) .

The generator also uses collection types that preserve insertion order at runtime. Maps
described by `additionalProperties` use
[`indexmap::IndexMap<String, T>`][rustdoc-indexmap-map] , and arrays with
`uniqueItems: true` use [`indexmap::IndexSet<T>`][rustdoc-indexmap-set] . For example,
the `tags` of a petstore `Pet` are `uniqueItems` , so the field is
`Option<indexmap::IndexSet<String>>` , and its `vaccinations` map is
`Option<indexmap::IndexMap<String, Vec<VaccineRecord>>>` . If you include these types in
an existing crate, add `indexmap` with its `serde` feature to your `Cargo.toml` . A
generated workspace manifest includes that dependency when needed.

A `uniqueItems` field also carries a `serde_with::SetPreventDuplicates` adapter, so a
JSON array with a repeated item fails to deserialize:

```rust
#[serde_as(as = "Option<serde_with::SetPreventDuplicates<_>>")]
pub tags: Option<indexmap::IndexSet<String>>,
```

Use this flag to choose standard library collections instead:

```text
--no-ordered-collections
```

Set `--no-ordered-collections` to opt out of the `indexmap` runtime types. Map schemas
then resolve to [`std::collections::HashMap<String, T>`][rustdoc-hashmap] , and
`uniqueItems` arrays resolve to [`Vec<T>`][rustdoc-vec] , the same type as a normal
array, and accept repeated items. Use this flag when your code cannot depend on
`indexmap` , or when you do not need JSON key and element order to survive a
deserialize-then-serialize round trip.

This flag does not change the declaration order of items in the generated source, such
as struct fields, enum variants, and operation methods.

## Field Defaults

The generator uses a property's `default` for construction and for missing JSON fields.
A `#[default(...)]` attribute sets the field's value in the type's
[`Default`][rustdoc-default] implementation, and a struct-level `#[serde(default)]`
tells Serde to take every missing field from that implementation during deserialization.

Let's look at the petstore's `Order` schema, which requires `petId` and gives `quantity`
and `complete` defaults. Here it is without its `shipDate` and `status` properties:

```json
"Order": {
  "type": "object",
  "required": ["petId"],
  "properties": {
    "id": { "type": "integer", "format": "int64", "readOnly": true },
    "petId": { "type": "string", "format": "uuid" },
    "quantity": { "type": "integer", "format": "int32", "minimum": 1, "maximum": 10, "default": 1 },
    "complete": { "type": "boolean", "default": false }
  }
}
```

The generated struct carries the defaults on its fields and a single Serde attribute on
the struct:

```rust
#[serde(default)]
pub struct Order {
    pub id: Option<i64>,
    #[serde(rename = "petId")]
    pub pet_id: uuid::Uuid,
    #[validate(range(min = 1i32, max = 10i32))]
    #[default(Some(1i32))]
    pub quantity: Option<i32>,
    /* ... */
    #[default(Some(false))]
    pub complete: Option<bool>,
}
```

Deserializing `{"petId": "..."}` gives `quantity` the value `Some(1)` and `complete` the
value `Some(false)` , the same values `Order::default()` holds.

The attribute applies to the whole struct, so it fills required fields too. JSON without
`petId` deserializes with the nil UUID from `Order::default()` instead of failing. A
struct only gets `#[serde(default)]` when at least one of its fields has a `default` or
a `const` , so required fields in other structs are still rejected when they're missing.

Client output also reads `null` in these structs' required fields as a default. See
[Missing and Null Fields](./client-generation.md#missing-and-null-fields) .

### Union Defaults

When an untagged union can represent the schema's `default` , the generator writes an
`impl Default` that constructs it. It checks variants in the same order as
deserialization. For example, `ContentRequest.image_size` defaults to `auto_2K` ,
selecting `Self::Preset(ImageSizePreset::Auto2k)` . A string default can produce
`Self::String("cheesecake".to_string())` .

Without a usable schema default, a union that derives `Default` selects the first
variant whose construction won't recurse back into the union. Eligible variants include
constants, unit variants, arrays, nullable types, and payloads that don't refer back to
the union.

The generator chooses this fallback in specification order, or sorted order with
`--enum-layout sorted` . Moving array or string variants for deserialization doesn't
change it. Nullable unions record the fallback too, in case a containing struct's
derived `Default` needs the union to implement `Default` .

A nullable union includes a `null` branch. Without a usable schema default, it starts
without a `Default` implementation. Properties with that union type use
[`Option<T>`][rustdoc-option] , including properties listed as required. If another
generated type needs the union to implement `Default` , the generator can derive it
using the recorded fallback variant.

### Defaults Beside References

A property can also set a `default` next to its `$ref` , as OpenAPI 3.1 allows. When the
reference names an enum, a `const` , or a union, the field's default uses that value
instead of the referenced schema's own `default` . The petstore's `ChatPreference`
schema does this:

```json
"fallback": { "$ref": "#/components/schemas/ChatModel", "default": "chat-1.1" }
```

This property becomes
`#[default(Some(ChatModel::Chat11))] pub fallback: Option<ChatModel>` , along with the
deserialization default described in [Field Defaults](#field-defaults) . A union's
default selects its matching variant, so `"default": "slow"` on a `oneOf` of `const`
values becomes `#[default(Some(Mode::Slow))]` . The generator ignores a `default` next
to any other reference, because it can't build that value for the referenced type. A
default that matches no variant of an enum or union, such as an object default on a
referenced or inline `oneOf` , gets no `#[default(...)]` attribute, so the field keeps
its plain `Default` instead of a placeholder that picks the first variant.

## Constant Properties

A property with `const` , or with a single-value `enum` , uses that value as its
default. Deserialization supplies it when the property is absent. The optional `object`
property of the petstore's `ChatPreference` becomes:

```rust
#[default(Some("chat.preference".to_string()))]
pub object: Option<String>,
```

The client keeps a different value it receives rather than replacing it, as
[Constant Properties](./client-generation.md#constant-properties) describes.

## Field Validation

Schemas often limit the values they accept: a string's length, a number's range, or an
array's size. The generator turns the limits it supports into
[`validator`][rustdoc-validate] attributes on the generated fields, and `validate()`
checks a value against them. The following table shows which attribute each schema
keyword produces:

| Schema keyword | Validation attribute |
|---|---|
| `minLength`, `maxLength` on a string | `length(min = ..., max = ...)` |
| `minItems`, `maxItems` on an array | `length(min = ..., max = ...)` |
| `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum` | `range(...)` |
| `pattern` | `regex(path = ...)`, naming a generated `static` that holds the compiled expression |
| `format: email` | `email` |
| `format: uri` or `format: url` | `url` |
| `multipleOf` | `custom(function = ...)`, calling a check in `oas3-gen-support` |
| A property whose own type has validation | `nested` |

A required string without `minLength` also gets `length(min = 1)` , so an empty string
fails validation. Strings with a `date` , `date-time` , `time` , `uuid` , `byte` , or
`binary` format don't get length or pattern checks.

The petstore's `CollarTelemetry` schema uses several of these keywords. Here's an
excerpt of the generated struct:

```rust
pub struct CollarTelemetry {
    #[serde(rename = "collarId")]
    #[validate(length(min = 1u64), regex(path = "REGEX_COLLAR_TELEMETRY_COLLAR_ID"))]
    pub collar_id: String,
    /* ... */
    /// Latitude and longitude
    #[validate(length(min = 2u64, max = 2u64))]
    pub location: Vec<f64>,
    /* ... */
    /// Validation: Must be a multiple of 5
    #[serde(rename = "batteryPercent")]
    #[validate(
        range(min = 0i32, max = 100i32),
        custom(function = oas3_gen_support::multiple_of::<5>)
    )]
    pub battery_percent: Option<i32>,
    /* ... */
    #[serde(rename = "ownerEmail")]
    #[validate(email)]
    pub owner_email: Option<String>,
    /* ... */
}
```

The `collarId` property is required and has a `pattern` , so `collar_id` gets both a
minimum length and a regular expression check. `batteryPercent` declares `minimum: 0` ,
`maximum: 100` , and `multipleOf: 5` . The same rules apply to a request body that isn't
an object. For example, the `text/plain` body of `setPetNotes` declares
`maxLength: 2000` , so the body field of `SetPetNotesRequest` carries
`length(max = 2_000u64)` .

### Multiples

A `multipleOf` check calls a function from `oas3-gen-support` . An integer field calls
`multiple_of::<N>` , which accepts a value that divides evenly by `N` . With
`multipleOf: 5` , `batteryPercent` accepts `85` and rejects `87` . A number field calls
`multiple_of_ratio::<NUMERATOR, DENOMINATOR>` , with the multiple written as a fraction:
`multipleOf: 0.25` becomes `multiple_of_ratio::<1, 4>` . That check applies only to
`f32` and `f64` fields. It accepts a value when the nearest multiple of the fraction,
rounded to the field's type, is the value itself. It also accepts a value whose quotient
by the fraction is within `1e-9` of a whole number, which absorbs rounding that
accumulates in arithmetic. So `0.3` counts as a multiple of `0.1` in either type, as do
`0.1 + 0.2` and a sum of one hundred `0.1` values, while `0.35` doesn't. Where the
field's type is coarser than the multiple, every value rounds to a multiple, so an `f32`
field with `multipleOf: 0.1` accepts `1_100_000.125` . Either check reports a failure as
a validation error with the code `multiple_of` .

Every `multipleOf` adds a line such as `Validation: Must be a multiple of 5` to the
field's documentation. These multiples get only that line:

- A fractional multiple on an integer field, such as `multipleOf: 0.5`.
- A multiple that can't be written as a fraction whose numerator and
  denominator fit in a [`u32`][rustdoc-u32], such as `1e-20`.

### When Validation Runs

Generated types derive
[`Validate`][rustdoc-validate] only when the API accepts them, such as request structs and request bodies. A schema used only in responses, such as `Pet`, keeps no validation attributes. Request builders validate when you call `build()`, as the [Builder Pattern](./builders.md)
chapter explains. [Making a Request](./client-generation.md#making-a-request) and
[Implementing the Service Trait](./server-generation.md#implementing-the-service-trait)
cover when the client and server check a request.

## OData Support

An OData API may omit metadata fields that its schema lists as required. If you
encounter this with fields such as `@odata.type` , use `--odata-support` to make
eligible fields optional in the generated types.

```text
--odata-support
```

This rule applies to names starting with `@odata.` when the parent schema has no
discriminator and isn't an intersection using `allOf` .

### Input Schema

Let's use a schema that requires an ID and two metadata fields:

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

### Default (OData Support Disabled)

Without the flag, all three fields are required strings:

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

With the flag, the metadata fields become optional:

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

Now deserialization succeeds when either metadata field is absent, and the corresponding
Rust field is `None` . The `id` field is still required because its name doesn't start
with `@odata.` .

## Type Customization

Use `--customize` to choose a `serde_with` adapter for a generated field. An adapter
controls how Serde reads or writes a value. The generated field keeps its Rust type, and
the adapter appears in its [`#[serde_as]`][rustdoc-serde-as] attribute.

```text
-c, --customize <TYPE=PATH>
```

The following keys select commonly customized formats:

| Key | OpenAPI Format | Rust Field Type |
|-----|----------------|-----------------|
| `date_time` | `date-time` | [`chrono::DateTime<chrono::Utc>`][rustdoc-chrono-datetime] |
| `date` | `date` | [`chrono::NaiveDate`][rustdoc-chrono-date] |
| `time` | `time` | [`chrono::NaiveTime`][rustdoc-chrono-time] |
| `duration` | `duration` | [`std::time::Duration`][rustdoc-duration] |
| `uuid` | `uuid` | [`uuid::Uuid`][rustdoc-uuid] |

The date-time type uses [`chrono::Utc`][rustdoc-chrono-utc] as its time zone. For these
examples, suppose your crate defines adapters named `DateTimeAdapter` and `DateAdapter`
. Each adapter must implement [`serde_with::SerializeAs`][rustdoc-serialize-as] or
[`serde_with::DeserializeAs`][rustdoc-deserialize-as] as needed by the generated field.
To use both adapters, repeat the flag:

```bash
cargo run -- generate types -i petstore.json -o types.rs \
  -c date_time=crate::DateTimeAdapter \
  -c date=crate::DateAdapter
```

### Default (No Customization)

The petstore's `CollarTelemetry` schema has a required `recordedAt` date-time and an
optional `recordedOn` date. Without an override, the fields use their normal Serde
implementations:

```rust
pub struct CollarTelemetry {
    /* ... */
    #[serde(rename = "recordedAt")]
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "recordedOn")]
    pub recorded_on: Option<chrono::NaiveDate>,
    /* ... */
}
```

### With a Date-Time Adapter

If we pass only `-c date_time=crate::DateTimeAdapter` , the generator adds an adapter to
`recorded_at` :

```rust
pub struct CollarTelemetry {
    /* ... */
    #[serde_as(as = "crate::DateTimeAdapter")]
    #[serde(rename = "recordedAt")]
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "recordedOn")]
    pub recorded_on: Option<chrono::NaiveDate>,
    /* ... */
}
```

Notice that `recorded_at` is still a
[`chrono::DateTime<chrono::Utc>`][rustdoc-chrono-datetime] . The `DateTimeAdapter`
controls its conversion during serialization and deserialization. The `recorded_on`
field is unchanged because we haven't supplied a `date` override in this example. The
generator also adds `#[serde_with::serde_as]` to any struct that uses an adapter.

### Handling Optional and Array Fields

The generator wraps the adapter in [`Option`][rustdoc-option] and [`Vec`][rustdoc-vec]
to match optional and array fields. For example, the same date-time override gives the
optional `homeVisit` property of `AdoptionRequest` this attribute:

```rust
pub struct AdoptionRequest {
    /* ... */
    #[serde_as(as = "Option<crate::DateTimeAdapter>")]
    #[serde(rename = "homeVisit")]
    pub home_visit: Option<chrono::DateTime<chrono::Utc>>,
}
```

An array of date-times would get `Vec<crate::DateTimeAdapter>` , and an optional array
`Option<Vec<crate::DateTimeAdapter>>` . Each attribute follows the shape of its field.
You supply the adapter for one date-time value; the generator adds the surrounding
container adapters.

[rustdoc-chrono-date]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveDate.html
[rustdoc-chrono-datetime]: https://docs.rs/chrono/0.4.45/chrono/struct.DateTime.html
[rustdoc-chrono-time]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveTime.html
[rustdoc-chrono-utc]: https://docs.rs/chrono/0.4.45/chrono/struct.Utc.html
[rustdoc-default]: https://doc.rust-lang.org/std/default/trait.Default.html
[rustdoc-deserialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.DeserializeAs.html
[rustdoc-duration]: https://doc.rust-lang.org/std/time/struct.Duration.html
[rustdoc-hashmap]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
[rustdoc-indexmap-map]: https://docs.rs/indexmap/2.14.2/indexmap/map/struct.IndexMap.html
[rustdoc-indexmap-set]: https://docs.rs/indexmap/2.14.2/indexmap/set/struct.IndexSet.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-serde-as]: https://docs.rs/serde_with/3.24.0/serde_with/attr.serde_as.html
[rustdoc-serialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.SerializeAs.html
[rustdoc-u32]: https://doc.rust-lang.org/std/primitive.u32.html
[rustdoc-uuid]: https://docs.rs/uuid/1.26.1/uuid/struct.Uuid.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html
