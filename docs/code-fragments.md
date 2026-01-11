# Code Generation Fragments

This document provides a complete reference of all code generation fragments in the `codegen/` module. Fragments are small, composable units that implement `ToTokens` to generate Rust source code from AST definitions.

## Fragment Architecture

The codegen stage transforms AST types (from `ast/`) into Rust source code using the `quote` crate. Each fragment encapsulates a specific code generation pattern and implements `ToTokens` for seamless composition.

```text
AST Types (ast/)           Fragments (codegen/)           Output
─────────────────          ──────────────────────         ──────────
StructDef          ───▶    StructFragment          ───▶   struct Foo { ... }
EnumDef            ───▶    EnumGenerator           ───▶   enum Bar { ... }
DiscriminatedEnumDef ─▶    DiscriminatedEnumGen.   ───▶   enum Baz { ... } + serde impls
ResponseEnumDef    ───▶    ResponseEnumGenerator   ───▶   enum Response { ... }
TypeAliasDef       ───▶    TypeAliasFragment       ───▶   type Alias = Target;
OperationInfo      ───▶    ClientGenerator         ───▶   impl Client { async fn ... }
```

## Fragment Hierarchy

### Entry Points

| Fragment | File | Purpose |
|----------|------|---------|
| `SchemaCodeGenerator` | `mod.rs:185` | Main orchestrator for all code generation |
| `ClientGenerator` | `client.rs:23` | HTTP client struct and method generation |
| `ServerGenerator` | `server.rs:11` | HTTP server trait generation (axum) |
| `ModFileGenerator` | `mod_file.rs:15` | Module file (`mod.rs`) generation |

### Struct Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `StructFragment` | `structs.rs:24` | Complete struct with impl blocks and header map |
| `StructDefinitionFragment` | `structs.rs:54` | Struct definition with derives, attrs, fields |
| `StructFieldFragment` | `structs.rs:100` | Individual field with docs, serde, validation |
| `StructImplBlockFragment` | `structs.rs:178` | Impl block with methods |
| `StructMethodFragment` | `structs.rs:232` | Individual struct method dispatch |

### Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `EnumGenerator` | `enums.rs:374` | Value enum with derives and optional case-insensitive deser |
| `EnumValueVariantFragment` | `enums.rs:163` | Single enum variant with docs, serde, default |
| `EnumVariants<T>` | `enums.rs:826` | Generic container for comma-separated variants |
| `EnumMethodsImplFragment` | `enums.rs:124` | Impl block with enum helper methods |
| `EnumMethodFragment` | `enums.rs:48` | Individual enum helper method |
| `DisplayImplFragment` | `enums.rs:256` | `Display` trait implementation |
| `DisplayImplArmFragment` | `enums.rs:220` | Single arm of Display match |
| `CaseInsensitiveDeserializeImplFragment` | `enums.rs:303` | Case-insensitive `Deserialize` impl |
| `CaseInsensitiveDeserializeArmFragment` | `enums.rs:288` | Single arm for case-insensitive matching |

### Discriminated Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `DiscriminatedEnumGenerator` | `enums.rs:683` | Tagged union with custom serde |
| `DiscriminatedVariantFragment` | `enums.rs:450` | Single variant with type reference |
| `DiscriminatorConstImplFragment` | `enums.rs:650` | `DISCRIMINATOR_FIELD` constant |
| `DiscriminatedDefaultImplFragment` | `enums.rs:475` | Default trait implementation |
| `DiscriminatedSerializeImplFragment` | `enums.rs:510` | Custom `Serialize` delegation |
| `DiscriminatedDeserializeImplFragment` | `enums.rs:567` | Custom `Deserialize` with discriminator lookup |
| `DiscriminatedDeserializeArmFragment` | `enums.rs:552` | Match arm for discriminator value |

### Response Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ResponseEnumGenerator` | `enums.rs:752` | HTTP response enum with optional axum impl |
| `ResponseEnumFragment` | `enums.rs:783` | Response enum definition |
| `ResponseVariantFragment` | `enums.rs:845` | Response variant with status doc |

### Method Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HelperMethodFragment<Parts>` | `methods.rs:16` | Generic helper method with vis, params, impl |
| `FieldFunctionParameterFragment` | `methods.rs:52` | Function parameter from field |
| `StructConstructorFragment` | `methods.rs:72` | Struct initialization with `..Default::default()` |
| `DefaultConstructorFragment` | `enums.rs:24` | `Type::default()` with optional Box |

### Builder Method Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `BuilderMethodFragment` | `structs.rs:624` | `#[builder] fn new(...)` with validation |
| `BuilderConstructionFragment` | `structs.rs:676` | Self construction with nested structs |

### Response Parsing Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ParseResponseMethodFragment` | `structs.rs:274` | `async fn parse_response(...)` method |
| `StatusCheckFragment` | `structs.rs:335` | `if status == X { ... }` block |
| `StatusConditionFragment` | `structs.rs:360` | Status check expression |
| `ResponseDispatchFragment` | `structs.rs:389` | Response handling dispatch |
| `ContentDispatchFragment` | `structs.rs:419` | Content-type based dispatch |
| `ContentCheckFragment` | `structs.rs:484` | Content-type check expression |
| `ResponseCaseFragment` | `structs.rs:513` | Single response variant construction |
| `ResponseExtractionFragment` | `structs.rs:550` | Response body extraction logic |
| `FallbackFragment` | `structs.rs:592` | Default/unknown response handling |

### Attribute Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `DeriveAttribute<T>` | `attributes.rs:34` | `#[derive(...)]` with BTreeSet |
| `generate_derives_from_slice` | `attributes.rs:54` | Derive generation from iterator |
| `generate_outer_attrs` | `attributes.rs:64` | Outer attribute generation |
| `generate_serde_attrs` | `attributes.rs:78` | Combined `#[serde(...)]` attribute |
| `generate_validation_attrs` | `attributes.rs:92` | Combined `#[validate(...)]` attribute |
| `generate_deprecated_attr` | `attributes.rs:102` | `#[deprecated]` attribute |
| `generate_serde_as_attr` | `attributes.rs:110` | `#[serde_as(...)]` attribute |
| `generate_doc_hidden_attr` | `attributes.rs:117` | `#[doc(hidden)]` attribute |
| `generate_field_default_attr` | `attributes.rs:125` | `#[default(...)]` attribute |
| `generate_docs_for_field` | `attributes.rs:11` | Field documentation with examples |

### Client Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ClientGenerator` | `client.rs:23` | Client struct, constructors, methods |
| `generate_method` | `client.rs:133` | Single async client method |
| `generate_http_init` | `client.rs:178` | HTTP method initialization |
| `generate_url_construction` | `client.rs:193` | URL building from path segments |
| `generate_query_params` | `client.rs:203` | `.query(&request.query)` chain |
| `generate_header_params` | `client.rs:215` | `.headers(...)` chain |
| `generate_body` | `client.rs:230` | Request body handling |
| `generate_multipart` | `client.rs:295` | Multipart form construction |
| `generate_response` | `client.rs:380` | Response parsing logic |
| `BodyResult` | `client.rs:13` | Body generation result with conditional flag |
| `ResponseHandling` | `client.rs:18` | Response type and parse logic |

### Server Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ServerGenerator` | `server.rs:11` | Server trait generation |
| `AxumIntoResponse` | `server.rs:54` | `IntoResponse` impl for response enums |
| `AxumIntoResponseVariant` | `server.rs:88` | Individual variant response conversion |

### Header Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HeaderMapGenerator` | `headers.rs:6` | `TryFrom<&Struct> for HeaderMap` impl |
| `generate_field_insertion` | `headers.rs:56` | Single header field insertion |
| `header_value_expr` | `headers.rs:82` | Header value expression |

### HTTP Status Code Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HttpStatusCode` | `http.rs:8` | `StatusCodeToken` to `http::StatusCode` |

### Constant Generation

| Function | File | Purpose |
|----------|------|---------|
| `generate_regex_constants` | `constants.rs:8` | Static `LazyLock<Regex>` constants |
| `generate_header_constants` | `constants.rs:57` | HTTP header name constants |

### Type Coercion

| Function | File | Purpose |
|----------|------|---------|
| `json_to_rust_literal` | `coercion.rs:7` | JSON value to Rust literal |
| `coerce_to_rust_type` | `coercion.rs:27` | Type-specific coercion |
| `coerce_to_string` | `coercion.rs:48` | String coercion |
| `coerce_to_bool` | `coercion.rs:64` | Boolean coercion |
| `coerce_to_int` | `coercion.rs:79` | Signed integer coercion |
| `coerce_to_uint` | `coercion.rs:93` | Unsigned integer coercion |
| `coerce_to_float` | `coercion.rs:107` | Float coercion |

### Type Alias Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `TypeAliasFragment` | `type_aliases.rs:8` | `type Alias = Target;` |

## Fragment Composition Pattern

Fragments compose hierarchically. For example, `StructFragment` composes:

```text
StructFragment
├── StructDefinitionFragment
│   ├── generate_derives_from_slice
│   ├── generate_outer_attrs
│   ├── generate_serde_attrs
│   └── StructFieldFragment (for each field)
│       ├── generate_docs_for_field
│       ├── generate_serde_as_attr
│       ├── generate_serde_attrs
│       ├── generate_validation_attrs
│       ├── generate_deprecated_attr
│       ├── generate_field_default_attr
│       └── generate_doc_hidden_attr
├── StructImplBlockFragment
│   └── StructMethodFragment (for each method)
│       ├── ParseResponseMethodFragment
│       │   ├── StatusCheckFragment
│       │   │   ├── StatusConditionFragment
│       │   │   └── ResponseDispatchFragment
│       │   │       ├── ResponseCaseFragment
│       │   │       │   └── ResponseExtractionFragment
│       │   │       └── ContentDispatchFragment
│       │   │           ├── ContentCheckFragment
│       │   │           └── ResponseCaseFragment
│       │   └── FallbackFragment
│       └── BuilderMethodFragment
│           └── BuilderConstructionFragment
└── HeaderMapGenerator
```

## ToTokens Trait Pattern

All fragments implement `ToTokens` for seamless composition:

```rust
impl ToTokens for StructFieldFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.field.name;
    let docs = generate_docs_for_field(&self.field);
    let type_tokens = &self.field.rust_type;
    let serde_attrs = generate_serde_attrs(&self.field.serde_attrs);
    // ... more attribute generation
    
    tokens.extend(quote! {
      #docs
      #serde_attrs
      #vis #name: #type_tokens
    });
  }
}
```

## HelperMethodParts Trait

Generic trait for method generation, used by both struct and enum methods:

```rust
pub(crate) trait HelperMethodParts {
  type Kind;
  fn method(&self) -> MethodNode<Self::Kind>;
  fn parameters(&self) -> impl ToTokens;
  fn implementation(&self) -> TokenStream;
}
```

Implementations:
- `EnumMethodFragment` for enum constructor methods
- Used by `HelperMethodFragment<Parts>` for generic method generation

## Key Design Decisions

1. **Small, focused fragments**: Each fragment handles one concern (e.g., a single attribute, a single variant)

2. **Composition over inheritance**: Fragments compose via field references, not inheritance hierarchies

3. **Visibility threading**: Most fragments accept `Visibility` to generate appropriate `pub`/`pub(crate)` modifiers

4. **BTreeMap/BTreeSet for determinism**: Regex lookups and derive collections use sorted collections

5. **Option-based conditional generation**: Many fragments return empty `quote! {}` for None/empty cases

6. **Context through Rc**: `CodeGenerationContext` wraps `CodegenConfig` and is shared via `Rc`
