# Code Generation Fragments

This document provides a complete reference of all code generation fragments in the `codegen/` module. Fragments are small, composable units that implement `ToTokens` to generate Rust source code from AST definitions.

## Fragment Architecture

The codegen stage transforms AST types (from `ast/`) into Rust source code using the `quote` crate. Each fragment encapsulates a specific code generation pattern and implements `ToTokens` for seamless composition.

```text
AST Types (ast/)            Fragments (codegen/)           Output
─────────────────           ──────────────────────         ──────────
Vec<RustType>       ───▶    TypesFragment           ───▶   Complete types.rs file
RustType            ───▶    TypeFragment            ───▶   (dispatches to specific fragment)
 StructDef          ───▶    StructFragment          ───▶   struct Foo { ... }
 EnumDef            ───▶    EnumFragment            ───▶   enum Bar { ... }
 DiscriminatedEnum  ───▶    DiscriminatedEnumFrag.  ───▶   enum Baz { ... } + serde impls
 ResponseEnumDef    ───▶    ResponseEnumFragment    ───▶   enum Response { ... } (client)
 ResponseEnumDef    ───▶    AxumResponseEnumFrag.   ───▶   enum + IntoResponse (server)
 TypeAliasDef       ───▶    TypeAliasFragment       ───▶   type Alias = Target;
Vec<OperationInfo>  ───▶    ClientFragment          ───▶   impl Client { async fn ... }
ServerRequestTrait  ───▶    ServerGenerator         ───▶   trait ApiServer + handlers + router
```

## Fragment Hierarchy

### Entry Points

| Fragment | File | Purpose |
|----------|------|---------|
| `SchemaCodeGenerator` | `mod.rs` | Main orchestrator: `generate_types`, `generate_client`, `generate_client_mod`, `generate_server_mod` |
| `TypesFragment` | `types.rs` | Types file generation (imports, constants, types) |
| `TypeFragment` | `types.rs` | Single `RustType` dispatch to specific fragment based on type |
| `ModuleUsesFragment` | `types.rs` | Grouped use statements generation |
| `UseFragment` | `types.rs` | Single `use module::{items}` statement |
| `ClientFragment` | `client.rs` | HTTP client struct and method generation |
| `ServerGenerator` | `server.rs` | HTTP server trait generation (axum) |
| `ModFileFragment` | `mod_file.rs` | Module file (`mod.rs`) generation with `mod` and `use` declarations |

### Struct Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `StructFragment` | `structs.rs` | Complete struct with impl blocks and header map |
| `StructDefinitionFragment` | `structs.rs` | Struct definition with derives, attrs, fields |
| `StructFieldFragment` | `structs.rs` | Individual field with docs, serde, validation |
| `StructImplBlockFragment` | `structs.rs` | Impl block with methods |
| `StructMethodFragment` | `structs.rs` | Individual struct method dispatch |

### Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `EnumFragment` | `enums.rs` | Value enum with derives and optional case-insensitive deser |
| `EnumValueVariantFragment` | `enums.rs` | Single enum variant with docs, serde, default |
| `EnumVariants<T>` | `enums.rs` | Generic container for comma-separated variants |
| `EnumMethodsImplFragment` | `enums.rs` | Impl block with enum helper methods |
| `EnumMethodFragment` | `enums.rs` | Individual enum helper method |
| `DisplayImplFragment` | `enums.rs` | `Display` trait implementation |
| `DisplayImplArmFragment` | `enums.rs` | Single arm of Display match |
| `CaseInsensitiveDeserializeImplFragment` | `enums.rs` | Case-insensitive `Deserialize` impl |
| `CaseInsensitiveDeserializeArmFragment` | `enums.rs` | Single arm for case-insensitive matching |

### Discriminated Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `DiscriminatedEnumFragment` | `enums.rs` | Tagged union with custom serde |
| `DiscriminatedVariantFragment` | `enums.rs` | Single variant with type reference |
| `DiscriminatorConstImplFragment` | `enums.rs` | `DISCRIMINATOR_FIELD` constant |
| `DiscriminatedDefaultImplFragment` | `enums.rs` | Default trait implementation |
| `DiscriminatedSerializeImplFragment` | `enums.rs` | Custom `Serialize` delegation |
| `DiscriminatedDeserializeImplFragment` | `enums.rs` | Custom `Deserialize` with discriminator lookup |
| `DiscriminatedDeserializeArmFragment` | `enums.rs` | Match arm for discriminator value |

### Response Enum Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ResponseEnumFragment` | `enums.rs` | Response enum definition (client mode) |
| `ResponseVariantFragment` | `enums.rs` | Response variant with status doc |
| `AxumResponseEnumFragment` | `server.rs` | Response enum with `IntoResponse` impl (server mode) |

### Method Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HelperMethodFragment<Parts>` | `methods.rs` | Generic helper method with vis, params, impl |
| `FieldFunctionParameterFragment` | `methods.rs` | Function parameter from field |
| `StructConstructorFragment` | `methods.rs` | Struct initialization with `..Default::default()` |
| `DefaultConstructorFragment` | `enums.rs` | `Type::default()` with optional Box |

### Builder Method Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `BuilderMethodFragment` | `structs.rs` | `#[builder] fn new(...)` with validation |
| `BuilderConstructionFragment` | `structs.rs` | Self construction with nested structs |

### Response Parsing Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ParseResponseMethodFragment` | `structs.rs` | `async fn parse_response(...)` method |
| `StatusCheckFragment` | `structs.rs` | `if status == X { ... }` block |
| `StatusConditionFragment` | `structs.rs` | Status check expression |
| `ResponseDispatchFragment` | `structs.rs` | Response handling dispatch |
| `ContentDispatchFragment` | `structs.rs` | Content-type based dispatch |
| `ContentCheckFragment` | `structs.rs` | Content-type check expression |
| `ResponseCaseFragment` | `structs.rs` | Single response variant construction |
| `ResponseExtractionFragment` | `structs.rs` | Response body extraction logic |
| `FallbackFragment` | `structs.rs` | Default/unknown response handling |

### Attribute Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `DeriveAttribute<T>` | `attributes.rs` | `#[derive(...)]` with BTreeSet |
| `generate_derives_from_slice` | `attributes.rs` | Derive generation from iterator |
| `generate_outer_attrs` | `attributes.rs` | Outer attribute generation |
| `generate_serde_attrs` | `attributes.rs` | Combined `#[serde(...)]` attribute |
| `generate_validation_attrs` | `attributes.rs` | Combined `#[validate(...)]` attribute |
| `generate_deprecated_attr` | `attributes.rs` | `#[deprecated]` attribute |
| `generate_serde_as_attr` | `attributes.rs` | `#[serde_as(...)]` attribute |
| `generate_doc_hidden_attr` | `attributes.rs` | `#[doc(hidden)]` attribute |
| `generate_field_default_attr` | `attributes.rs` | `#[default(...)]` attribute |
| `generate_docs_for_field` | `attributes.rs` | Field documentation with examples |

### Client Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ClientFragment` | `client.rs` | Entry point: Client struct, constructors, methods |
| `ClientStructFragment` | `client.rs` | Client struct definition (`struct ApiClient { client, base_url }`) |
| `ClientDefaultImplFragment` | `client.rs` | Default trait impl for client |
| `ClientConstructorsFragment` | `client.rs` | `new()`, `with_base_url()`, `with_client()` methods |
| `ClientMethodFragment` | `client.rs` | Single async operation method |
| `HttpInitFragment` | `client.rs` | HTTP method initialization (`self.client.get(url)`) |
| `UrlConstructionFragment` | `client.rs` | URL building from path segments |
| `QueryParamsFragment` | `client.rs` | `.query(&request.query)` chain |
| `HeaderParamsFragment` | `client.rs` | `.headers(...)` chain |
| `RequestBodyFragment` | `client.rs` | Body handling dispatch by content type |
| `SimpleBodyFragment` | `client.rs` | Simple body chains (json, form, text, binary) |
| `XmlBodyFragment` | `client.rs` | XML body handling with Content-Type header |
| `MultipartFormFragment` | `client.rs` | Multipart form construction |
| `MultipartStrictFragment` | `client.rs` | Typed multipart fields |
| `MultipartFallbackFragment` | `client.rs` | JSON serialization fallback for multipart |
| `MultipartFieldFragment` | `client.rs` | Single multipart field addition |
| `ResponseParsingFragment` | `client.rs` | Response handling dispatch by content type |

### Server Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `ServerGenerator` | `server.rs` | Entry point: trait, handlers, router generation |
| `ServerTraitFragment` | `server.rs` | `trait ApiServer: Send + Sync { ... }` definition |
| `ServerTraitMethodFragment` | `server.rs` | Single trait method signature |
| `HandlerFunctionFragment` | `server.rs` | Axum handler function for each operation |
| `ExtractorsFragment` | `server.rs` | Handler parameter extractors (State, Path, Query, HeaderMap, body) |
| `BodyExtractorFragment` | `server.rs` | Body extractor based on content type (Json, Form, String, Bytes) |
| `RequestConstructionFragment` | `server.rs` | Request struct construction from extractors |
| `RouterFragment` | `server.rs` | `fn router<S>(service: S) -> Router` generation |
| `HttpMethodFragment` | `server.rs` | HTTP method to axum routing function (get, post, etc.) |
| `AxumResponseEnumFragment` | `server.rs` | Response enum with `IntoResponse` impl |
| `AxumIntoResponse` | `server.rs` | `IntoResponse` impl for response enums |
| `AxumIntoResponseVariant` | `server.rs` | Individual variant response conversion |

### Header Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HeaderMapFragment` | `headers.rs` | `TryFrom<&Struct> for HeaderMap` impl |
| `HeaderFieldInsertionFragment` | `headers.rs` | Single header field insertion |
| `header_value_expr` | `headers.rs` | Header value expression (helper function) |

### HTTP Status Code Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `HttpStatusCode` | `http.rs` | `StatusCodeToken` to `http::StatusCode` |

### Constant Generation Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `RegexConstantFragment` | `constants.rs` | Single `static REGEX_*: LazyLock<Regex>` constant |
| `RegexConstantsResult` | `constants.rs` | Collection of regex constants with lookup map |
| `HeaderConstantsFragment` | `constants.rs` | Collection of HTTP header name constants |

### Type Coercion

| Function | File | Purpose |
|----------|------|---------|
| `json_to_rust_literal` | `coercion.rs` | JSON value to Rust literal |
| `coerce_to_rust_type` | `coercion.rs` | Type-specific coercion |
| `coerce_to_string` | `coercion.rs` | String coercion |
| `coerce_to_bool` | `coercion.rs` | Boolean coercion |
| `coerce_to_int` | `coercion.rs` | Signed integer coercion |
| `coerce_to_uint` | `coercion.rs` | Unsigned integer coercion |
| `coerce_to_float` | `coercion.rs` | Float coercion |

### Type Alias Fragments

| Fragment | File | Purpose |
|----------|------|---------|
| `TypeAliasFragment` | `type_aliases.rs` | `type Alias = Target;` |

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
└── HeaderMapFragment
    └── HeaderFieldInsertionFragment (for each field)
        └── header_value_expr
```

Similarly, `ClientFragment` composes:

```text
ClientFragment
├── ClientStructFragment
├── ClientDefaultImplFragment
├── ClientConstructorsFragment
└── ClientMethodFragment (for each HTTP operation)
    ├── UrlConstructionFragment
    │   └── ParsedPath (already implements ToTokens)
    ├── HttpInitFragment
    ├── QueryParamsFragment
    ├── HeaderParamsFragment
    ├── RequestBodyFragment
    │   ├── SimpleBodyFragment (json, form, text, binary)
    │   ├── XmlBodyFragment
    │   └── MultipartFormFragment
    │       ├── MultipartStrictFragment
    │       │   └── MultipartFieldFragment (for each field)
    │       └── MultipartFallbackFragment
    └── ResponseParsingFragment
```

And `ServerGenerator` composes:

```text
ServerGenerator
├── ServerTraitFragment
│   └── ServerTraitMethodFragment (for each operation)
├── HandlerFunctionFragment (for each operation)
│   ├── ExtractorsFragment
│   │   └── BodyExtractorFragment
│   └── RequestConstructionFragment
└── RouterFragment
    └── HttpMethodFragment (for each route method)
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
