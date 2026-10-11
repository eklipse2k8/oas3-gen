# Enums and Unions

OpenAPI can restrict a value to a list or allow it to match several schemas. The
generator represents these choices with Rust enums. This chapter covers variant names,
ordering, serialization, discriminators, and values outside a known list.

Most examples come from `fixtures/petstore.json` ; smaller standalone schemas are shown
where useful. Code excerpts omit imports and some definitions. For field defaults and
validation, see [Schemas and Types](./schema-types.md) .

- [Enum Mode](#enum-mode)
- [Enum Layout](#enum-layout)
- [Numeric-Backed Enums](#numeric-backed-enums)
- [Boolean and Mixed-Type Enums](#boolean-and-mixed-type-enums)
- [Discriminated Unions](#discriminated-unions)
- [Constant Unions](#constant-unions)
- [Non-Exhaustive Enums](#non-exhaustive-enums)
- [Helper Methods](#helper-methods)

## Enum Mode

Different schema values can become the same Rust variant name. For example, `"ACTIVE"`
and `"active"` both become `Active` . Use `--enum-mode` to choose whether to merge those
values, keep them separate, or accept differences in ASCII letter case during
deserialization.

```text
--enum-mode <MODE>
```

| Value | Behavior |
|-------|----------|
| `merge` (default) | Merge duplicates; first occurrence is canonical, others become aliases |
| `preserve` | Keep all variants; append numeric suffix to collisions |
| `relaxed` | Merge duplicates; enable case-insensitive deserialization |

### Input Schema

Let's use the same schema to compare the three modes:

```json
{
  "type": "string",
  "enum": ["ACTIVE", "active", "Active", "PENDING"]
}
```

### Example: `--enum-mode merge`

The default mode merges values that produce the same Rust identifier. It uses the first
value as the serialization name and adds the others as Serde aliases for
deserialization:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE", alias = "active", alias = "Active")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}
```

All three spellings of `"ACTIVE"` in the schema deserialize to `Status::Active` . The
lowercase string `"pending"` produces an error because it isn't declared as a value or
alias. Merging preserves the declared spellings; it doesn't make every spelling
case-insensitive.

### Example: `--enum-mode preserve`

Use `preserve` when your application needs to distinguish the declared spellings. Each
value gets its own variant, and colliding names get numeric suffixes:

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

Now `"ACTIVE"` , `"active"` , and `"Active"` deserialize to `Status::Active` ,
`Status::Active1` , and `Status::Active2` , respectively. Your code can match those
variants separately.

### Example: `--enum-mode relaxed`

Use `relaxed` when you want to accept values regardless of ASCII letter case. The
generator merges the variants and writes a [`Deserialize`][rustdoc-serde-deserialize]
implementation that converts input to ASCII lowercase before matching:

```rust
#[derive(Debug, Clone, Serialize)]
pub enum Status {
    #[serde(rename = "ACTIVE", alias = "active", alias = "Active")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}

impl<'de> serde::Deserialize<'de> for Status {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.to_ascii_lowercase().as_str() {
            "active" => Ok(Self::Active),
            "pending" => Ok(Self::Pending),
            _ => Err(serde::de::Error::unknown_variant(&s, &["ACTIVE", "PENDING"])),
        }
    }
}
```

This version accepts both `"PENDING"` and `"pending"` as `Status::Pending` , as well as
the different spellings of `"ACTIVE"` . Serialization still uses the declared name shown
in the `#[serde(rename)]` attribute.

## Enum Layout

Once you've chosen how to handle enum values, you can choose their order in the
generated source. The `--enum-layout` flag preserves the specification's order by
default or sorts variants by their Rust names.

```text
--enum-layout <LAYOUT>
```

| Value | Behavior |
|-------|----------|
| `spec` (default) | Preserve variant order from the OpenAPI document |
| `sorted` | Sort variants alphabetically by Rust variant name |

`sorted` applies to value enums (string `enum` ), `oneOf` /`anyOf` union variants, and
discriminated enum variants. HTTP response (status-code) enums are unaffected and
continue to use status-code order.

Use `sorted` when you want the declarations to stay in the same order after the
specification reorders enum values. This can make changes to the generated source easier
to review.

Ordering can also affect the default value. For value enums that derive
[`Default`][rustdoc-default] , the generator marks the variant matching the schema's
`default` . If no variant matches, it uses the first declared variant. Switching to
`sorted` can therefore change the fallback default.

Untagged unions, described with `oneOf` or `anyOf` , always place a variant that wraps a
plain `String` last, after any sorting. Deserialization tries an untagged union's
variants in order, and a `String` accepts every JSON string. The petstore's
`ChatModelChoice` lists `{ "type": "string" }` before a `$ref` to the `ChatModel` enum,
but its generated `ChatModel(ChatModel)` variant comes first, so `"chat-1.0"` decodes as
`ChatModel(ChatModel::Chat10)` .

Variants that wrap an array come first. The petstore's `ArrayOrSingle` lists a
`TextBlock` before an array of them, and the generated enum tries the array first:

```rust
#[serde(untagged)]
pub enum ArrayOrSingle {
    Array(Vec<TextBlock>),
    #[default]
    TextBlock(TextBlock),
}
```

The array's move doesn't change the default: `TextBlock` keeps `#[default]` .

For default selection in unions and fields that refer to them, see
[Union Defaults](./schema-types.md#union-defaults) .

### Input Schema

The petstore's `Species` schema lists its values in an order that isn't alphabetical,
and it has no `default` . Its `x-enum-descriptions` extension documents each value:

```json
"Species": {
  "type": "string",
  "description": "Kinds of pets the store cares for",
  "enum": ["dog", "cat", "bird", "fish", "reptile", "small_mammal"],
  "x-enum-descriptions": [
    "a dog",
    "a cat",
    "a bird",
    "a fish",
    "a reptile",
    "a rabbit, guinea pig, or other small mammal"
  ]
}
```

### Example: `--enum-layout spec`

The default layout keeps `Dog` first, followed by the remaining values in schema order.
Because `Dog` is the first declared variant, it's also the default:

```rust
/// Kinds of pets the store cares for
pub enum Species {
    /// a dog
    #[serde(rename = "dog")]
    #[default]
    Dog,
    /// a cat
    #[serde(rename = "cat")]
    Cat,
    /// a bird
    #[serde(rename = "bird")]
    Bird,
    /* ... */
    /// a rabbit, guinea pig, or other small mammal
    #[serde(rename = "small_mammal")]
    SmallMammal,
}
```

### Example: `--enum-layout sorted`

The sorted layout puts `Bird` first. The serialization names and descriptions remain
attached to the same variants, but the fallback default moves to `Bird` :

```rust
/// Kinds of pets the store cares for
pub enum Species {
    /// a bird
    #[serde(rename = "bird")]
    #[default]
    Bird,
    /// a cat
    #[serde(rename = "cat")]
    Cat,
    /// a dog
    #[serde(rename = "dog")]
    Dog,
    /* ... */
    /// a rabbit, guinea pig, or other small mammal
    #[serde(rename = "small_mammal")]
    SmallMammal,
}
```

## Numeric-Backed Enums

An enum can represent numbers as well as strings. When a schema combines an `enum` array
with `type: integer` or `type: number` , the generated type reads and writes JSON
numbers. The generator does the same when a schema lists only numbers and declares no
`type` , choosing `i64` for whole numbers and `f64` when any value has a fractional
part. The generator supplies [`Serialize`][rustdoc-serde-serialize] and
[`Deserialize`][rustdoc-serde-deserialize] implementations so that a value such as
`8000` stays a number during serialization. The petstore's `Priority` schema,
`{ "enum": [1, 2, 3] }` , is an example without a `type` .

Consider the petstore's `SampleRate` schema, which lists the sample rates the assistant
accepts:

```json
"SampleRate": {
  "description": "Audio sample rate in Hz (integer-backed enum)",
  "type": "integer",
  "enum": [8000, 16000, 24000, 44100, 48000],
  "default": 24000
}
```

The generator emits one variant per value, a serializer that turns each variant into its
number, and a deserializer that maps numbers back to variants. The schema's `default`
selects `Value24000` for [`Default`][rustdoc-default] . Serialization uses
[`serde::Serializer`][rustdoc-serde-serializer] , and deserialization uses
[`serde::Deserializer`][rustdoc-serde-deserializer] with
[`serde::de::Error`][rustdoc-serde-de-error] for invalid values:

```rust
/// Audio sample rate in Hz (integer-backed enum)
#[derive(Debug, Clone, PartialEq, Eq, Hash, oas3_gen_support::Default)]
pub enum SampleRate {
    Value8000,
    Value16000,
    #[default]
    Value24000,
    Value44100,
    Value48000,
}

impl serde::Serialize for SampleRate {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let value: i64 = match self {
            Self::Value8000 => 8000i64,
            /* ... */
            Self::Value48000 => 48000i64,
        };
        serializer.serialize_i64(value)
    }
}

impl<'de> serde::Deserialize<'de> for SampleRate {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        match value {
            8000i64 => Ok(Self::Value8000),
            /* ... */
            _ => Err(serde::de::Error::custom(/* ... */)),
        }
    }
}
```

`SampleRate::Value8000` now serializes to `8000` , and `8000` deserializes back to
`SampleRate::Value8000` . A number outside the listed values produces an error that
names the accepted values.

The schema's type and format determine which Rust number type the generated
implementation uses. Signed integers use [`i64`][rustdoc-i64] , unsigned integers use
[`u64`][rustdoc-u64] , and floating-point values use [`f64`][rustdoc-f64] . In each
case, the serialized value is a JSON number.

### Floating-Point Values

For a `number` enum, the generated deserializer reads an [`f64`][rustdoc-f64] and
compares its bit pattern with each allowed value. Here's an excerpt for the petstore's
`PlaybackRate` , which accepts `0.5` , `1.0` , `1.5` , and `2.0` :

```rust
impl<'de> serde::Deserialize<'de> for PlaybackRate {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        match value.to_bits() {
            bits if bits == (0.5f64).to_bits() => Ok(Self::Value0_5),
            /* ... */
            _ => Err(serde::de::Error::custom(/* ... */)),
        }
    }
}
```

The Rust variants don't store floating-point values: each variant identifies one allowed
number. These enums can therefore derive [`Eq`][rustdoc-eq] and [`Hash`][rustdoc-hash] ,
and you can use them as map keys or set members.

## Boolean and Mixed-Type Enums

An `enum` array can also list booleans, or mix strings, numbers, and booleans. Each
listed value keeps its JSON type on the wire, so a `true` in the schema is written and
read as the JSON boolean `true` , never as the string `"true"` .

The petstore's `Toggle` schema, `{ "type": "boolean", "enum": [true, false] }` ,
produces `True` and `False` variants with a serializer that writes a JSON boolean and a
deserializer that reads one:

```rust
impl<'de> serde::Deserialize<'de> for Toggle {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = bool::deserialize(deserializer)?;
        if value { Ok(Self::True) } else { Ok(Self::False) }
    }
}
```

When the listed values mix JSON types, as in the petstore's `ResponseQuality` schema,
`"enum": ["auto", 1, 0.5, true, 1e-6, 1e21, 18446744073709552000]` , each variant writes
its own type, and the deserializer reads a
[`serde_json::Value`][rustdoc-serde-json-value] and matches it against every listed
value:

```rust
impl serde::Serialize for ResponseQuality {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Value1 => serializer.serialize_i64(1i64),
            Self::Value0_5 => serializer.serialize_f64(0.5f64),
            Self::True => serializer.serialize_bool(true),
        }
    }
}
```

`ResponseQuality::Value1` reads the JSON number `1` . Query parameters and path segments
contain text. Numeric and boolean variants therefore also accept strings that parse to
their values: `1` selects `Value1` , `0.5` selects `Value0_5` , and `1e-6` selects the
variant for `0.000001` . Boolean text matches exactly, or case-insensitively under
[`--enum-mode relaxed`](#example---enum-mode-relaxed) , where `TRUE` selects `True` .
Listed strings are tried first, so in `["1", 1]` the string `"1"` selects the string
variant while the number `1` still selects the number variant. This applies to JSON too:
when the enum doesn't list the string `"1"` , the JSON string `"1"` decodes to the
number variant.

A declared `type` takes precedence over the listed values. With `"type": "string"` ,
every value is read and written as a string.

## Discriminated Unions

A `oneOf` lists alternative schemas for one value. When it declares a `discriminator` ,
one property of each object names its branch, and the generator produces an enum that
reads that property to choose a variant. The petstore's `ContentBlock` schema uses its
`type` property this way, mapping `text` to `TextBlock` , `image` to `ImageBlock` , and
so on. Here's an excerpt of the generated enum:

```rust
/// A union of content block types with discriminator
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
}

impl<'de> serde::Deserialize<'de> for ContentBlock {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get("type").and_then(|v| v.as_str()) {
            Some("text") => {
                serde_json::from_value(value)
                    .map(Self::Text)
                    .map_err(serde::de::Error::custom)
            }
            /* ... */
            None => Err(serde::de::Error::missing_field("type")),
            Some(other) => { /* ... */ }
        }
    }
}
```

The deserializer reads the whole object, looks up `type` , and decodes the object as the
matching branch. An object without `type` produces a missing-field error, and an
unlisted value produces an error that names it. To serialize, the enum writes the
wrapped struct, whose constant `type` field supplies the tag, as
[Constant Properties](./schema-types.md#constant-properties) describes.

### Inferred Discriminators

Some documents leave out the `discriminator` even though each branch fixes a property to
its own constant. The petstore's `UsageCounterA` and `UsageCounterB` schemas differ only
in their `type` constants. Here's `UsageCounterA` , trimmed of its `title` and `default`
keywords:

```json
"UsageCounterA": {
  "type": "object",
  "required": ["output_tokens", "type"],
  "properties": {
    "output_tokens": { "type": "integer", "minimum": 0 },
    "type": { "type": "string", "const": "counter_a" }
  }
}
```

`UsageCounterB` is the same apart from `"const": "counter_b"` . The items of
`UsageCounters` are an `anyOf` of these two schemas with no `discriminator` .

The generator infers a discriminator when a `oneOf` or `anyOf` meets all of these
conditions:

- It declares no `discriminator`.
- It has at least two branches other than `null`, and each is a `$ref` to an
  object schema.
- Every such branch requires the same property and sets it to a string
  `const`.
- No two branches use the same constant.

The generated enum uses that property to select a variant:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum UsageCounterUsageCounterKind {
    A(UsageCounterA),
    B(UsageCounterB),
}

impl<'de> serde::Deserialize<'de> for UsageCounterUsageCounterKind {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get("type").and_then(|v| v.as_str()) {
            Some("counter_a") => {
                serde_json::from_value(value)
                    .map(Self::A)
                    .map_err(serde::de::Error::custom)
            }
            Some("counter_b") => {
                serde_json::from_value(value)
                    .map(Self::B)
                    .map_err(serde::de::Error::custom)
            }
            /* ... */
        }
    }
}
```

The object `{"type": "counter_b", "output_tokens": 3}` decodes as
`UsageCounterUsageCounterKind::B` . If any condition isn't met, such as when a branch is
inline, the union stays untagged.

### Nullable Arrays of Unions

`UsageCounters` itself pairs an inline array with `null` :

```json
"UsageCounters": {
  "anyOf": [
    {
      "type": "array",
      "items": {
        "anyOf": [
          { "$ref": "#/components/schemas/UsageCounterA" },
          { "$ref": "#/components/schemas/UsageCounterB" }
        ]
      }
    },
    { "type": "null" }
  ],
  "default": null
}
```

The generator keeps the array shape. `UsageCounters` becomes an alias for a
[`Vec`][rustdoc-vec] of the item union, and a field that uses it carries the nullability
as an [`Option`][rustdoc-option] , as the `iterations` field of `ToolResultBlock` shows:

```rust
pub type UsageCounters = Vec<UsageCounterUsageCounterKind>;

pub struct ToolResultBlock {
    /* ... */
    pub iterations: Option<UsageCounters>,
    /* ... */
}
```

Any `anyOf` or `oneOf` that pairs a single inline array with `null` converts this way,
whatever the array's items are.

## Constant Unions

A `oneOf` or `anyOf` whose every branch is a `const` describes the same set of values as
an `enum` array, so the generator produces the same value enum. Each `const` becomes a
unit variant documented with its branch's `description` , and the schema's `default`
marks the matching variant. The petstore's `ResponseKind` schema is one of these:

```json
"ResponseKind": {
  "description": "A union of constants, generated as a value enum",
  "anyOf": [
    { "type": "string", "const": "text", "description": "Plain text" },
    { "type": "string", "const": "json" }
  ],
  "default": "json"
}
```

```rust
/// A union of constants, generated as a value enum
pub enum ResponseKind {
    /// Plain text
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "json")]
    #[default]
    Json,
}
```

Constants of other JSON types keep their types, as described in
[Boolean and Mixed-Type Enums](#boolean-and-mixed-type-enums) , and `null` branches make
the property optional instead of adding a variant.

A union can also mix `const` branches with branches that carry a payload, such as an
object or a string. Its `const` variants stay tagged so that each matches its wire
value, and they come first. Only the payload variants are marked `#[serde(untagged)]` .
The petstore's `UsageChoice` lists a string, a `$ref` to the `Usage` object, and the
constant `"auto"` , which is also its default:

```rust
pub enum UsageChoice {
    #[serde(rename = "auto")]
    #[default]
    Auto,
    #[serde(untagged)]
    Usage(Usage),
    #[serde(untagged)]
    String(String),
}
```

Here `"auto"` deserializes to `UsageChoice::Auto` and serializes back to `"auto"` . An
object becomes `UsageChoice::Usage` , and any other string becomes `UsageChoice::String`
. A schema `default` that names a constant, like `"auto"` , selects its variant.

## Non-Exhaustive Enums

An API can accept strings beyond a list of known values. An `anyOf` combining a string
enum with a freeform string describes this pattern. The generator creates one enum
containing the known variants and a variant for other strings.

Consider the petstore's schema for a model name. It pairs a freeform string with the
`ChatModel` enum, which lists `chat-1.0` and `chat-1.1` :

```json
"ChatModelId": {
  "description": "A listed chat model or any other model name",
  "anyOf": [{ "type": "string" }, { "$ref": "#/components/schemas/ChatModel" }]
}
```

The generator emits one variant per listed value, followed by an `Other` variant that
holds any other string. It marks the enum `#[non_exhaustive]` :

```rust
/// A listed chat model or any other model name
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, oas3_gen_support::Default)]
#[non_exhaustive]
pub enum ChatModelId {
    #[serde(rename = "chat-1.0")]
    #[default]
    Chat10,
    #[serde(rename = "chat-1.1")]
    Chat11,
    #[serde(untagged)]
    Other(String),
}
```

The generator copies the referenced values into the open enum. `ChatModel` itself stays
exhaustive, so a field typed `ChatModel` still rejects other values.

Deserialization checks the listed values first, so `"chat-1.0"` becomes
`ChatModelId::Chat10` . A value outside the list, such as `"chat-2.0"` , becomes
`ChatModelId::Other("chat-2.0".to_string())` instead of producing an error. Because
`Other` is `#[serde(untagged)]` , it serializes as the bare string, so an unlisted value
you received is sent back unchanged. The [`Display`][rustdoc-display] implementation
writes the same string. You construct a listed value through its variant, such as
`ChatModelId::Chat11` , and the enum still derives [`Eq`][rustdoc-eq] and
[`Hash`][rustdoc-hash] .

`ChatModelId::Other("chat-1.0".to_string())` serializes like `ChatModelId::Chat10` but
doesn't compare equal to it, so reserve `Other` for values outside the list. When a
model name comes from your own code, such as a configuration file, convert it with the
generated `From<String>` implementation instead of building `Other` yourself:

```rust
impl From<String> for ChatModelId {
    fn from(value: String) -> Self {
        match value.as_str() {
            "chat-1.0" => Self::Chat10,
            "chat-1.1" => Self::Chat11,
            _ => Self::Other(value),
        }
    }
}
```

`ChatModelId::from(name)` selects the named variant for each known value and `Other` for
values outside the list. For example, `"chat-1.0"` becomes `ChatModelId::Chat10` , so
comparing the result with that variant succeeds.

The
[`#[non_exhaustive]`][reference-non-exhaustive] attribute matters when the generated types live in their own crate, as with [workspace crate output](./generation-options.md#workspace-crate-output)
. A `match` in another crate must then include a wildcard arm. When a later version of
the specification lists another value, regenerating adds a variant without breaking that
`match` . Inside the crate that defines the enum, the attribute has no effect.

The generator applies these rules to open string enums:

- The listed values can also be written inline, as in
  `{ "type": "string", "enum": ["chat-1.0", "chat-1.1"] }`.
- If a listed value such as `"other"` already produces an `Other` variant, the
  catch-all becomes `Other2(String)` so the two stay distinct. The `tier`
  property of `ChatPreference` lists `standard`, `priority`, and `other`, so
  its `ChatPreferenceTier` enum ends with `Other` and `Other2(String)`.
- A schema `default` outside the list becomes the enum's
  [`Default`][rustdoc-default]. The same `tier` property declares
  `"default": "flex"`, so `ChatPreferenceTier::default()` returns
  `ChatPreferenceTier::Other2("flex".to_string())`.
- With [`--enum-mode relaxed`](#example---enum-mode-relaxed), listed values and
  their aliases match regardless of ASCII letter case, in deserialization and in
  `From<String>`. Anything else lands in `Other` with its original spelling.
- Where the generator writes a [`FromStr`][rustdoc-from-str] implementation, as
  it does for server output and response headers, parsing delegates to
  `From<String>`, so a value outside the list returns `Other` instead of an
  error.
- The listed values must all be strings. When the list includes numbers or
  booleans, the schema becomes an untagged union instead.
- The generator handles only `anyOf` this way. A `oneOf` that mixes a freeform
  string with listed values remains an untagged union, which tries its `String`
  variant last, as described in [Enum Layout](#enum-layout).

## Helper Methods

By default, the generator adds constructor helpers for supported enum variants. For a
variant that wraps a struct with a [`Default`][rustdoc-default] implementation, a helper
can accept the required values and fill in the remaining fields. Pass `--no-helpers` if
you'd prefer to construct the variants yourself.

```text
--no-helpers
```

### Default (Helpers Enabled)

For the petstore's `ContentBlock` enum, the helpers construct the nested structs and
wrap them in the matching variants:

```rust
/// A union of content block types with discriminator
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
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

Each helper takes the one field its struct requires, and `..Default::default()` fills in
the rest, including the constant `type` field. `ToolUseBlock` and `ToolResultBlock` each
require more than one field, so their variants get no helper.

The `text()` helper lets us supply the text directly:

```rust
let block = ContentBlock::text("Hello, world!".to_string());
```

### With `--no-helpers`

The enum still has the same variants, but the constructor helpers are omitted:

```rust
/// A union of content block types with discriminator
#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(TextBlock),
    Image(ImageBlock),
    Code(CodeBlock),
    ToolUse(ToolUseBlock),
    ToolResult(ToolResultBlock),
}
```

We now construct the `TextBlock` and pass it to the `Text` variant ourselves:

```rust
let block = ContentBlock::Text(TextBlock {
    text: "Hello, world!".to_string(),
    ..Default::default()
});
```

[reference-non-exhaustive]: https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute
[rustdoc-default]: https://doc.rust-lang.org/std/default/trait.Default.html
[rustdoc-display]: https://doc.rust-lang.org/std/fmt/trait.Display.html
[rustdoc-eq]: https://doc.rust-lang.org/std/cmp/trait.Eq.html
[rustdoc-f64]: https://doc.rust-lang.org/std/primitive.f64.html
[rustdoc-from-str]: https://doc.rust-lang.org/std/str/trait.FromStr.html
[rustdoc-hash]: https://doc.rust-lang.org/std/hash/trait.Hash.html
[rustdoc-i64]: https://doc.rust-lang.org/std/primitive.i64.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-serde-de-error]: https://docs.rs/serde/1.0.229/serde/de/trait.Error.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-deserializer]: https://docs.rs/serde/1.0.229/serde/trait.Deserializer.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-serde-serializer]: https://docs.rs/serde/1.0.229/serde/trait.Serializer.html
[rustdoc-u64]: https://doc.rust-lang.org/std/primitive.u64.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html
