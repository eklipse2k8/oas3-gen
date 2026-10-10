# Shared Generation Options

<a id="code-generation"></a>

Client and server generation use the same schemas and many of the same options.
This chapter explains those shared rules: choosing output modes, representing
responses, interpreting security requirements, and changing the generated types.
Use the [Flag Summary](#flag-summary) when you need a reminder of an option.

For a walkthrough of calling an API, start with
[Client Generation](./client-generation.md). To implement an API with Axum,
start with [Server Generation](./server-generation.md). Both chapters link back
here when an option applies to both targets.

The commands use `cargo run --` from a checkout of this repository. If you
installed the tool with Cargo, replace `cargo run --` with `oas3-gen`. Input
names such as `spec.json` and `petstore.json` refer to OpenAPI documents in your
current directory; substitute the path to your own document.

Unless a section says otherwise, the examples come from
`crates/oas3-gen/fixtures/petstore.json`. It describes a hypothetical pet store
and uses every OpenAPI feature the generator supports at least once. When a
section names an operation, such as `listPets`, or a schema, such as `Pet`,
you'll find it in that document.

The Rust examples are excerpts that focus on the option being discussed. They
omit imports, unrelated attributes, and some definitions. An ellipsis marks
omitted code, so those excerpts aren't complete programs.

## Table of Contents

- [Generation Modes](#generation-modes)
- [Operations and Parameters](#operations-and-parameters)
- [Responses](#responses)
- [Multipart Form Bodies](#multipart-form-bodies)
- [Security Schemes](#security-schemes)
- [Workspace Crate Output](#workspace-crate-output)
- [Generated Names](#generated-names)
- [Field Validation](#field-validation)
- [Visibility](#visibility)
- [Enum Mode](#enum-mode)
- [Enum Layout](#enum-layout)
- [Numeric-Backed Enums](#numeric-backed-enums)
- [Boolean and Mixed-Type Enums](#boolean-and-mixed-type-enums)
- [Discriminated Unions](#discriminated-unions)
- [Constant Unions](#constant-unions)
- [Non-Exhaustive Enums](#non-exhaustive-enums)
- [Field Defaults](#field-defaults)
- [Constant Properties](#constant-properties)
- [Helper Methods](#helper-methods)
- [OData Support](#odata-support)
- [Type Customization](#type-customization)
- [Operation Filtering](#operation-filtering)
- [Function Name Overrides](#function-name-overrides)
- [API Name Override](#api-name-override)
- [Schema Filtering](#schema-filtering)
- [Header Emission](#header-emission)
- [Builder Generation](#builder-generation)
- [Ordering and Collections](#ordering-and-collections)
- [Documentation Formatting](#documentation-formatting)
- [Known Limitations](#known-limitations)
- [Flag Summary](#flag-summary)

## Generation Modes

The positional `mode` argument selects the kind of output. Choose `types`
when you need data types, `client` or `client-mod` when you want to call an API,
and `server-mod` when you want to implement one:

```text
cargo run -- generate <MODE> -i spec.json -o <OUTPUT>
```

### `types`

The `types` mode writes type definitions to a single file. Let's generate the
petstore's types:

```bash
cargo run -- generate types -i petstore.json -o types.rs
```

Among others, `types.rs` now contains the `PetStatus` enum and the `Pet`
struct:

```rust
/// Where the pet is in the adoption process
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, oas3_gen_support::Default)]
pub enum PetStatus {
    #[serde(rename = "available")]
    #[default]
    Available,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "adopted")]
    Adopted,
}

/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /// Kinds of pets the store cares for
    pub species: Species,
    /// Where the pet is in the adoption process
    pub status: Option<PetStatus>,
    /* ... */
}
```

The required properties `id`, `name`, and `species` become plain fields, and
an optional property such as `status` becomes an [`Option`][rustdoc-option].
The derives follow how the API uses each type. `Pet` appears only in
responses, so it derives [`Deserialize`][rustdoc-serde-deserialize] but not
[`Serialize`][rustdoc-serde-serialize]. `NewPet`, which the API only accepts,
derives `Serialize` and [`Validate`][rustdoc-validate] instead. Server output
reverses those directions.

### `client`

The `client` mode writes types and a `reqwest` client to one file. See
[Choosing the Output Layout](./client-generation.md#choosing-the-output-layout)
for an example.

### `client-mod`

The `client-mod` mode writes `types.rs`, `client.rs`, and `mod.rs` to a directory.
See [Client Generation](./client-generation.md) for how to generate and use it.

### `server-mod`

The `server-mod` mode writes `types.rs`, `server.rs`, and `mod.rs` to a directory.
See [Server Generation](./server-generation.md) for how to implement the trait
and use the generated Axum router.

## Operations and Parameters

Each operation under `paths` becomes a client method, a server trait method,
and a server route, named after its `operationId` in `snake_case`. The
operation's parameters and request body travel together in a request struct
named after it. For example, `showPetById` becomes `show_pet_by_id()`, which
takes a `ShowPetByIdRequest` whose `path` and `header` fields hold its path and
header parameters.

Only server output includes the operations under `webhooks`. The petstore's
`petAdopted` webhook becomes the trait method `pet_adopted()` and the route
`/webhooks/petAdopted`. The client doesn't get a method for it.

This section covers shared Path Items, query arrays, cookie parameters, and
deprecation.

### Path Item References

OpenAPI 3.1 lets an entry in `paths` or `webhooks` reference a reusable Path
Item in `components.pathItems` instead of declaring its operations in place.
The petstore declares the vaccination endpoints this way:

```json
"/pets/{petId}/vaccinations": { "$ref": "#/components/pathItems/PetVaccinations" }
```

The `PetVaccinations` component declares the `petId` parameter, a `get`
operation named `listVaccinations`, and a `post` operation named
`recordVaccination`. The generator follows the reference and generates those
operations as though they were written under `paths`. The client gets
`list_vaccinations()` and `record_vaccination()` methods, and the server gets
both trait methods and their route. The `inventoryLow` webhook references the
`InventoryLowWebhook` component in the same way and becomes the server's
`inventory_low()` method.

A Path Item component can itself reference another component. The generator
follows the chain, and fields written next to a `$ref`, such as a `summary`,
take precedence over the fields of the component it names. Only references to
`#/components/pathItems/` in the same document are followed. An entry that
names a missing component, or a chain that loops back on itself, generates no
operations.

### Query Parameters

OpenAPI's default serialization for query parameters is the `form` style with
`explode: true`. For an array, that means one `name=value` pair per item, with
the name repeated. The petstore's `searchPets` operation declares its `tags`
filter without a `style`, so it uses that default:

```json
{
  "name": "tags",
  "in": "query",
  "description": "Pets carrying any of these tags, one `tags=` pair per tag",
  "schema": { "type": "array", "items": { "type": "string" } }
}
```

The generated query struct holds the array as a [`Vec`][rustdoc-vec]:

```rust
pub struct SearchPetsRequestQuery {
    /* ... */
    /// Pets carrying any of these tags, one `tags=` pair per tag
    pub tags: Option<Vec<String>>,
    /// Pets in any of these statuses, one `statuses=` pair per status
    pub statuses: Option<Vec<PetStatus>>,
    /* ... */
}
```

When `tags` holds `calm` and `shy`, the query carries `tags=calm&tags=shy`.
[Query Arrays](./client-generation.md#query-arrays) shows how the client sends
these values, and [Connecting the Router](./server-generation.md#connecting-the-router)
shows how the server reads them.

With `explode: false`, the items share a single pair. They're joined by a
comma for `form`, a space for `spaceDelimited`, or `|` for `pipeDelimited`,
and the generator adds a `serde_with` adapter that joins and splits them. The
petstore's `listPets` declares its `tags` filter with `style: form` and
`explode: false`:

```rust
pub struct ListPetsRequestQuery {
    /* ... */
    /// Only return pets that carry every listed tag
    #[serde_as(as = "Option<oas3_gen_support::StringWithCommaSeparator>")]
    pub tags: Option<Vec<String>>,
}
```

The same two tags now travel as one `tags` pair whose value is `calm,shy`. A
tag that contains a comma arrives as two tags.

### Cookie Parameters

A parameter declared with `in: cookie` travels in the `Cookie` header. The
request struct holds an operation's cookie parameters in a nested struct in its
`cookie` field. The petstore's `getPetCard` operation declares two optional
cookies, `cardTheme` and `recentlyViewed`. This excerpt omits the structs'
attributes:

```rust
pub struct GetPetCardRequest {
    pub path: GetPetCardRequestPath,
    pub cookie: GetPetCardRequestCookie,
}

pub struct GetPetCardRequestCookie {
    /// The visitor's preferred card theme
    pub card_theme: Option<GetPetCardRequestCookieCardTheme>,
    /// Pets the visitor viewed recently, most recent first
    pub recently_viewed: Option<Vec<uuid::Uuid>>,
}
```

Cookie parameters use OpenAPI's `form` style. An array's items are joined with
commas into one value, and each `name=value` pair is percent-encoded with the
[`cookie`][rustdoc-cookie] crate. [Cookie Parameters](./client-generation.md#cookie-parameters)
shows how the client sends them, and
[Receiving Cookie Parameters](./server-generation.md#receiving-cookie-parameters)
shows how the server reads them.

A cookie that carries a credential belongs in an `apiKey` security scheme with
`in: cookie`, as the petstore's `SessionCookie` does. See
[Security Schemes](#security-schemes).

### Deprecated Operations

An operation marked `deprecated: true` is still generated. Its client method
carries the [`#[deprecated]`][reference-deprecated] attribute, as
[Deprecated Operations](./client-generation.md#deprecated-operations) shows. A
deprecated schema property, such as the `tag` property of `Pet`, gets
`#[deprecated]` on its field in both outputs.

## Responses

Generated operations share a response enum named `ApiResponse`. Its variants
represent the status codes declared by the selected operations. When both
success and failure payloads are needed, the enum has the form
`ApiResponse<Value, Failure>`. In module output, you'll find it in `types.rs`.

The generator also adds an `Other` variant when an operation has no `default`
response. That variant lets the client return an undeclared status and its raw
body. The following table shows the possible variant shapes:

| Variant shape | When |
|---|---|
| `Ok(Value)`, `Created(Value)`, ... | A 2xx status that carries a body in at least one operation |
| `NotFound(Failure)`, `Conflict(Failure)`, ... | Any other declared status that carries a body |
| `NoContent`, `NotModified`, ... | A status that has no body, and no [headers](#response-headers) in its class, in any operation |
| `ClientError(http::StatusCode, Failure)` | A range such as `4XX`; the concrete status travels with the body |
| `Unknown(http::StatusCode, Failure)` | The spec's `default` response, when it declares a body |
| `Other(http::StatusCode, Vec<u8>)` | A status the operation does not declare when it has no `default` response |

The variants that carry a status use [`http::StatusCode`][rustdoc-http-status]. `Other` holds its raw
body in a [`Vec<u8>`][rustdoc-vec].

Each operation supplies its own types for `Value` and `Failure`. We'll call
these the two *response classes*: `Value` covers 2xx statuses, and `Failure`
covers the remaining statuses. Before accounting for headers, the body types
determine each parameter as follows:

| Bodies declared in the class | Parameter |
|---|---|
| None | [`()`][rustdoc-unit] |
| One type | That type |
| One type plus a status that carries a payload but no body | [`Option<Type>`][rustdoc-option] |
| Several types | An untagged payload union such as `ErrorOrProblem` |
| Several types plus a status that carries a payload but no body | `Option<Union>` |

A payload union is an enum that can hold one of several body types. For
example, `showPetById` answers `404` with a `Problem` and its `4XX` and
`default` responses with an `Error`, so its `Failure` is `ErrorOrProblem`.
Operations that declare the same set of body types share that union definition. If no
status uses one of the response enum's type parameters, the generator omits
that parameter. If a schema already uses the name `ApiResponse`, the generator
names the response enum `ApiResponseType`.

### Response Headers

Some operations return headers as well as a body. When any status in a
response class declares headers, the class uses `WithHeaders<Headers, Body>`
as its parameter. `Headers` is a generated struct with one field for each
header declared across that class. `Body` is the type chosen using the table
above.

The wrapper keeps the headers and body together. The generator adds it beside
the response enum only when at least one response declares headers. If a
schema already uses the name `WithHeaders`, the generated name gets a numeric
suffix:

```rust
/// A response body together with the headers its response declares.
#[derive(Debug, Clone)]
pub struct WithHeaders<Headers, Body> {
    pub headers: Headers,
    pub body: Body,
}
```

A header field is optional unless every status in its class marks it as
required. If an optional header is missing or can't be parsed, its value is
`None`. For a list of values, one invalid element makes the whole list `None`.
A missing or invalid required response header causes `parse_response` to
return an error.

Operations with the same header fields share a header struct. The generator
names that struct after its headers, as in `XNextHeaders` or
`ETagAndXCacheHeaders`, and adds a numeric suffix if the name is already in
use. A status without a body can still carry headers: its variant then holds
a `WithHeaders` value whose body is [`()`][rustdoc-unit] or an [`Option`][rustdoc-option].

The generator ignores `Content-Type` entries in response header definitions.
A header without a schema uses [`String`][rustdoc-string]. Enums used in headers implement
[`FromStr`][rustdoc-from-str] for both client and server output, and response headers receive name
constants in the same way as request headers.

For example, generating the whole petstore produces this response enum and,
for the pagination headers of `listPets`, this header struct:

```rust
/// Response enum shared by every operation.
///
/// `Value` is the operation's success body type and `Failure` its error body type.
/// A status code is a unit variant when no operation gives it a body.
#[derive(Debug, Clone)]
pub enum ApiResponse<Value, Failure> {
    ///200
    Ok(Value),
    ///201
    Created(Value),
    ///202
    Accepted(Value),
    ///204
    NoContent(Value),
    ///304
    NotModified,
    ///400
    BadRequest(Failure),
    ///401
    Unauthorized(Failure),
    ///404
    NotFound(Failure),
    ///409
    Conflict(Failure),
    ///422
    UnprocessableEntity(Failure),
    ///4XX
    ClientError(http::StatusCode, Failure),
    ///default
    Unknown(http::StatusCode, Failure),
    ///Status code the operation does not declare; carries the raw body.
    Other(http::StatusCode, Vec<u8>),
}

/// Response headers that share one status class in an operation.
#[derive(Debug, Clone, PartialEq, oas3_gen_support::Default)]
pub struct XNextAndXTotalCountAndXRequestIdHeaders {
    /// A link to the next page of results
    pub x_next: Option<String>,
    /// Number of pets that match the filters
    pub x_total_count: Option<i64>,
    /// Echo of the request's correlation identifier
    pub x_request_id: Option<uuid::Uuid>,
}
```

A status is a unit variant only when no operation declares a body or headers
for it. No petstore operation returns a `204` body, but `describePets`
declares an `Allow` header on its `204` response, so `NoContent` carries a
`Value`. `NotModified` has neither, so it's a unit variant.

The `listPets` operation returns
`ApiResponse<WithHeaders<XNextAndXTotalCountAndXRequestIdHeaders, Pets>, Error>`.
The client decodes that value from an HTTP response, and the server constructs
it to send a response. See [Reading Responses](./client-generation.md#reading-responses) and
[Returning Responses](./server-generation.md#returning-responses) for examples.

### Response Media Types

Each status declares its body under a media type, and that media type decides
the body's Rust type:

| Media type | Body type |
|---|---|
| JSON, including `+json` types such as `application/problem+json` | The type generated from the schema |
| `text/*`, such as `text/plain` | [`String`][rustdoc-string] |
| A binary type, such as `image/png` or `application/octet-stream` | [`Vec<u8>`][rustdoc-vec] |
| `text/event-stream` | A stream of events, described below |

A text body is read as the text it is, when its type is a string or bytes, and
otherwise parsed with `FromStr`, the inverse of the `Display` a server writes it
with. A text body whose type has no text form, such as an object, is decoded
from JSON, which is how a server sends it.

For example, `getPetCard` returns its kennel card as `text/plain`, so it
returns `ApiResponse<String, Problem>`. `getPetPhoto` returns `image/png` and
produces `ApiResponse<Vec<u8>, Problem>`. Both declare their `404` body as
`application/problem+json`, which decodes into the `Problem` struct.

[Reading Responses](./client-generation.md#reading-responses) shows how the
client decodes these bodies, and
[Returning Responses](./server-generation.md#returning-responses) shows how the
server sends them.

A `text/event-stream` response delivers Server-Sent Events. The client reads
it as `oas3_gen_support::EventStream<oas3_gen_support::SseFrame<T>>` and the
server returns `futures::stream::BoxStream<'static, T>`, where `T` is the type
of one event's data. The petstore's `streamPetVitals` operation streams
`VitalsReading` values this way. When the schema describes the whole event,
with properties drawn from `data`, `event`, `id`, and `retry`, it's a frame
type: the client reads it as an alias of `SseFrame`, the server sends it as one
complete Axum event, and the stream carries that type itself. In a payload
union, a stream member is named after its item type with a `Stream` suffix,
such as `ContentResponseOrAssistantEventFrameStream`.
See [Reading Server-Sent Events](./client-generation.md#reading-server-sent-events)
for the client side and
[Streaming Server-Sent Events](./server-generation.md#streaming-server-sent-events)
for the server side.

## Multipart Form Bodies

When an operation's request body uses `multipart/form-data`, the body schema
still becomes an ordinary struct in `types.rs`. The difference is on the wire:
the generated client sends each property as its own part, named after the
property, and the generated server reads those parts back into the struct.
This section explains which part each property becomes, so you can predict
what a server receives.

Let's start with a body that uploads files next to a few text fields. The
petstore's `submitIntakeForm` operation sends the `IntakeForm` schema as
`multipart/form-data`. Here it is, trimmed to four of its properties and the
requirements on them:

```json
"IntakeForm": {
  "type": "object",
  "description": "A shelter intake form",
  "required": ["name", "attachments"],
  "properties": {
    "name": { "type": "string" },
    "file": {
      "anyOf": [{ "type": "string", "format": "binary" }, { "type": "null" }],
      "description": "The previous owner's records"
    },
    "attachments": { "type": "array", "items": { "type": "string", "format": "binary" } },
    "quality": { "$ref": "#/components/schemas/CoatQuality" }
  }
}
```

The generator produces a struct with `name: String`, `file: Option<Vec<u8>>`,
`attachments: Vec<Vec<u8>>`, and `quality: Option<CoatQuality>`. The full
schema also has base64 text, numbers, dates, nested objects, and maps, and the
operation's `encoding` map changes how several of them are sent. The rest of
this section describes the parts those fields become. See
[Sending Multipart Bodies](./client-generation.md#sending-multipart-bodies) and
[Receiving Multipart Bodies](./server-generation.md#receiving-multipart-bodies)
for the code that writes and reads them.

### Choosing the Part for Each Property

The following table shows how the generator encodes each kind of property when
the media type has no `encoding` entry for it:

| Property schema | Part sent |
|---|---|
| `type: string` with `format: binary` | A file part labelled `application/octet-stream`, carrying the raw bytes |
| No `type` at all, such as `{}` | A file part, because OpenAPI 3.1 reads an untyped multipart property as raw binary |
| `type: string`, or a string `enum` | A text part holding the string |
| `type: string` with `format: byte` | A text part holding the base64 text |
| `type: integer`, `number`, `boolean`, or an integer `enum` | A text part holding the value, such as `3` or `true` |
| `format: date`, `date-time`, `time`, or `uuid` | A text part holding the serialized value, such as `2026-10-07T16:42:52Z` |
| `type: object`, a `$ref` to one, or a discriminated `oneOf` | An `application/json` part |
| An untagged `anyOf` or `oneOf`, or a schema such as `additionalProperties: true` | A text part when the value is a string, and an `application/json` part otherwise |
| `type: array` | One part per item, each encoded by the rules above |
| `additionalProperties` | One part per entry, named by its key |

An untyped property is generated as [`Vec<u8>`][rustdoc-vec] in a multipart body,
and `items: {}` produces `Vec<Vec<u8>>`. That's how you upload any number of
files under one name, as in the specification's own example. The petstore's
`uploadMedia` operation uses both forms: its untyped `cover` property becomes
`Option<Vec<u8>>`, and its `file` property, an array with `items: {}`, becomes
`Option<Vec<Vec<u8>>>`. The generator
makes this change only when the schema is used just as a multipart body. If
the same schema is also a JSON response or another schema's property, the field
stays [`serde_json::Value`][rustdoc-serde-json-value], so its JSON form doesn't
change, and the generator warns.

A value whose type depends on the data, such as an untagged union, goes as
JSON unless it's a string. That way a server can tell the string `"5"` from the
number `5`.

### Encoding Objects

A media type's `encoding` map adjusts individual properties. Its
`contentType` field labels a property's parts:

- When the field lists several media types, such as `image/png, image/jpeg`,
  the generator uses the first one, because a part can carry only one. It warns
  when it labels every file with the first type of a list.
- A wildcard such as `image/*` describes a range rather than a type a client
  can send, so the generator skips it, falls back to the default, and warns.
  `IntakeForm` labels its `thumbnail` file this way, so the file is sent as
  `application/octet-stream`.
- A JSON media type serializes the value as JSON. A string property with
  `contentType: application/json` is therefore sent as `"hello"`, quotes
  included.
- Objects are always serialized as JSON. When `contentType` names a non-JSON
  type for an object, such as `application/xml`, the part keeps
  `application/json`, and the generator warns.
- Rust strings are UTF-8, so a `charset` other than UTF-8 on a text part
  becomes `charset=utf-8`, with a warning.

The `style`, `explode`, and `allowReserved` fields switch a property to the
serialization used for query parameters. When any of them is present,
`contentType` is ignored for text values, and values aren't percent-encoded.
File properties keep their file parts. The following table applies to struct
and map properties:

| Property | Encoding | Parts sent |
|---|---|---|
| Array | `explode: true` (the default for `form`) | One part per item |
| Array | `explode: false` | One part, with items joined by `,`, a space, or `\|` for `form`, `spaceDelimited`, or `pipeDelimited` |
| Object | `style: deepObject` | One part per member, named `property[member]` |
| Object | `style: form`, `explode: true` | One part per member, named by the member |
| Object | `style: form`, `explode: false` | One part of alternating names and values, such as `min,1,max,5` |

`IntakeForm` uses several of these rows. Its `ids` array is `pipeDelimited`
with `explode: false`, `filter` uses `deepObject`, `range` uses `form` with
`explode: false`, and `extras` uses `form` with `explode: true`.

This serialization has no way to escape its delimiters. An item that contains
the delimiter, such as `a,b` in a comma-joined array, arrives as two items. A
`style` value that `multipart/form-data` doesn't support, such as `matrix`,
falls back to `form` with a warning.

An Encoding Object can also declare `headers` for a part. The generated client
doesn't send part headers, and the generated server doesn't expose them. Part
headers are optional unless a header sets `required: true`, so the generator
warns about each required one.

### Generation Warnings

The generator also warns about specifications it can't follow exactly:

- An `encoding` entry names a property the body schema doesn't have.
- The body schema isn't an object with properties, such as a top-level
  `oneOf`. Its parts come from its JSON form, so binary values aren't sent as
  files.
- The media type is another `multipart` subtype, such as `multipart/mixed`.
  The client sends `multipart/form-data`.
- A property name contains a double quote or a line break, which a part header
  can't carry.
- An array holds nested arrays, whose inner arrays are sent as JSON text.

The parser the generator uses doesn't keep a schema's `contentEncoding`, so a
`type: string` property with `contentEncoding: base64` is sent as a plain text
part rather than labelled `application/octet-stream`.

## Security Schemes

OpenAPI describes authentication in two places. A *security scheme* under
`components.securitySchemes` describes a credential and how it travels in a
request. A *security requirement* in `security` names the schemes an operation
accepts. The generator supports `apiKey` schemes in headers, query parameters,
and cookies, and `http` schemes that use bearer tokens.

Let's start with a header key. This excerpt from the petstore declares a scheme
named `ApiKeyAuth` and uses it as the default requirement:

```json
{
  "security": [{ "ApiKeyAuth": [] }],
  "components": {
    "securitySchemes": {
      "ApiKeyAuth": {
        "type": "apiKey",
        "in": "header",
        "name": "X-Api-Key",
        "description": "Key issued in the Petstore partner dashboard."
      }
    }
  }
}
```

The scheme name, `ApiKeyAuth`, identifies the credential in generated Rust
names. The `name` value, `X-Api-Key`, identifies the HTTP header. Operations
inherit the top-level `security` unless they declare their own requirements.

A bearer scheme is an `http` scheme whose `scheme` value is `bearer`. It has no
`name`, because the token always travels in the `Authorization` header as
`Bearer <token>`. The petstore declares one named `BearerAuth`:

```json
{
  "components": {
    "securitySchemes": {
      "BearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
    }
  }
}
```

The generator matches `bearer` in any case, as HTTP does. A `bearerFormat`
value, such as `JWT`, only documents the token, so it doesn't change the
generated code. A header parameter named `Authorization` doesn't produce
credentials; OpenAPI says to ignore it, as [Header Emission](#header-emission)
explains.

### Requirements

The entries in the `security` array are alternatives: a request needs to
satisfy one entry. Within an entry, all the named schemes are required. These
petstore operations, most of them under `/staff/keys`, show how the grouping
changes the requirement:

| Operation's `security` | Requirement | Petstore operation |
|---|---|---|
| `[{"ApiKeyAuth": []}]`, inherited from the top level | Supply the header key. | `listApiKeys` |
| `[{"QueryKey": []}, {"SessionCookie": []}]` | Supply a query key or a session cookie. | `createApiKey` |
| `[{"ApiKeyAuth": [], "SessionCookie": []}]` | Supply both the header key and the session cookie. | `getApiKey` |
| `[{"BearerAuth": []}, {"ApiKeyAuth": []}]` | Supply a bearer token or the header key. | `revokeApiKey` |
| `[{}, {"ApiKeyAuth": []}]` | Allow anonymous access, or accept the header key. | `listPets` |
| `[]` or `[{}]` | Allow anonymous access, replacing any top-level requirement. | `getHealth` |

`QueryKey` is an `apiKey` scheme read from the `api_key` query parameter, and
`SessionCookie` is one read from the `session` cookie. `BearerAuth` is the
bearer scheme shown earlier.

The server represents a credential as required when every alternative needs
it. Otherwise, the field is optional, and the extractor checks that the
supplied credentials complete at least one alternative. These checks establish
that credentials are present. Your service still needs to decide whether their
values authorize the request.

Other schemes, such as HTTP basic authentication and OAuth 2.0, don't produce
generated credentials. The generator reports a warning for each referenced
scheme it can't handle. If an operation has an alternative containing one of
these schemes, all its credential fields become optional, including those in
that same alternative. The generated extractor can't enforce the full
requirement, so your application must handle authorization for that operation.
In the petstore, `replaceApiKey` accepts `BasicAuth` or `ApiKeyAuth`, and
`adoptPet` accepts OAuth 2.0, OpenID Connect, or `ApiKeyAuth`. Both operations
receive the header key as an optional field.

### Keeping Keys Out of Logs

The server stores API keys and bearer tokens as
[`secrecy::SecretString`][rustdoc-secret-string], and the client uses the same
type for header keys, query keys, and bearer tokens. Its debug representation
shows `[REDACTED]` in place of the value, and it clears its own stored value
when dropped. It doesn't implement [`Display`][rustdoc-display],
[`Serialize`][rustdoc-serde-serialize], or [`PartialEq`][rustdoc-partial-eq].

To read a `SecretString`, call `expose_secret()` with
[`secrecy::ExposeSecret`][rustdoc-expose-secret] in scope; the
[server example](./server-generation.md#reading-and-checking-keys) shows how.
Once exposed, the returned string is ordinary text, so keep it out of log output.

[Sending Credentials](./client-generation.md#sending-credentials) shows how the
client stores and sends each scheme, and
[Receiving Credentials](./server-generation.md#receiving-credentials) shows the
server's credential types and rejections.

## Workspace Crate Output

Use `--workspace` when you want the output to have its own `Cargo.toml` and
`src/` directory. You can build that output as a crate or include it in a Cargo
workspace. The flag is available with `client-mod` and `server-mod`; using it
with `types` or `client` produces a generation error.

```text
-w, --workspace
```

For example, let's generate a client crate in `petstore-api`:

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace
```

The generated directory has this structure:

```text
petstore-api/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── types.rs
    └── client.rs
```

Notice that the source files are inside `src/`, and the module root is
`src/lib.rs`. This makes `petstore-api` a crate root. The type and client or
server files contain the same code as the corresponding module output.

### Crate Name

The package takes its name from the final component of the output directory, so
`-o petstore-api` produces `name = "petstore-api"` and a `petstore_api` library
target. Names are transliterated to ASCII and converted to kebab-case:
`-o "output/Pet Store"` yields `pet-store`, `-o café-api` yields `cafe-api`, and
`-o petstore_v2` yields `petstore-v2`. The library target of `petstore-v2` is
still `petstore_v2`, because Cargo maps dashes to underscores.

Generation fails when no valid name can be derived, for example when the
directory name is empty after sanitization or starts with a digit.

### Generated Manifest

The manifest declares the package and the dependencies its generated code
uses. The command above writes this manifest for the petstore client, shown
here without its banner; the dependency list for your API may differ:

```toml
[package]
name = "petstore-api"
version = "0.0.0"
edition = "2024"
rust-version = "1.89"
description = "Rust client generated from the Petstore OpenAPI document"

[dependencies]
anyhow = "1.0"
chrono = { version = "0.4.42", default-features = false, features = ["std", "clock", "serde"] }
http = "1.5"
indexmap = { version = "2.14", features = ["serde"] }
oas3-gen-support = "0.29.0"
regex = "1.13"
reqwest = { version = "0.13", default-features = false, features = ["form", "json", "multipart", "http2", "native-tls", "query", "stream"] }
reqwest_cookie_store = "0.10"
secrecy = "0.10"
serde = { version = "1.0", features = ["derive"] }
serde_html_form = { version = "0.4", default-features = false, features = ["ser", "std"] }
serde_json = { version = "1.0", features = ["preserve_order"] }
serde_with = { version = "3.24", features = ["base64", "chrono"] }
uuid = { version = "1.26", features = ["serde"] }
validator = { version = "0.21", features = ["derive", "indexmap"] }
```

The description comes from the document's `info.title`, `Petstore`. Adding
`--enable-builders` adds a `bon` entry. Generating the petstore server with
`server-mod` instead leaves out `reqwest`, `reqwest_cookie_store`, and
`serde_html_form`, and adds the Axum crates:

```toml
axum = { version = "0.8.5", features = ["multipart"] }
axum-extra = { version = "0.12", features = ["cookie", "query"] }
```

The generator rewrites `Cargo.toml` on every run and adds a banner identifying
it as generated output. Put code that needs its own dependencies in a separate
crate so that regenerating the API doesn't overwrite your changes.

The package version starts at `0.0.0`. Pass `--module-version` to set a version
for the generated crate:

```bash
cargo run -- generate client-mod -i petstore.json -o petstore-api --workspace --module-version 1.0.0
```

The `--module-version` flag requires `--workspace`, because that's the mode
that creates the manifest.

### Dependencies

Each generator release contains the dependency version requirements it writes
to the manifest. It doesn't read them from your project's environment. The
requirement for `oas3-gen-support` uses the generator's version. Cargo resolves
these requirements when you build the generated crate.

The manifest declares crates referenced by the generated source. For example,
a specification without date or time types doesn't need a `chrono` entry, and
server output doesn't need the client's `reqwest` dependency. The following
table describes the dependencies you may see:

| Crate | Declared when the generated code |
|-------|----------------------------------|
| `anyhow` | returns [`anyhow::Result`][rustdoc-anyhow-result] from client or server methods |
| `axum` | defines a `server-mod` router and extractors, with the `multipart` feature |
| `axum-extra` | reads query parameters or cookie API keys on the server, with the `cookie` and `query` features |
| `bon` | derives builders (`--enable-builders`) |
| `chrono` | maps `date`, `date-time`, or `time` formats |
| `futures` | returns or sends a server's `text/event-stream` as a [`BoxStream`][rustdoc-futures-boxstream] |
| `http` | emits header constants or [`HeaderMap`][rustdoc-http-header-map] conversions |
| `indexmap` | emits [`IndexMap`][rustdoc-indexmap-map]/[`IndexSet`][rustdoc-indexmap-set] collection types |
| `oas3-gen-support` | uses runtime derives, diagnostics, `multipleOf` checks, or reads event streams on the client |
| `regex` | emits `pattern` validation constants |
| `reqwest` | performs `client-mod` HTTP calls; the `form` feature sends `application/x-www-form-urlencoded` bodies |
| `reqwest_cookie_store` | sends cookie API keys or encodes cookie parameters from the client |
| `secrecy` | stores API keys and bearer tokens |
| `serde` | derives [`Serialize`][rustdoc-serde-serialize]/[`Deserialize`][rustdoc-serde-deserialize] |
| `serde_html_form` | serializes query parameters on the client, with only its `ser` and `std` features |
| `serde_json` | handles freeform [`serde_json::Value`][rustdoc-serde-json-value] payloads |
| `serde_with` | applies `serde_as` conversions |
| `uuid` | maps the `uuid` format |
| `validator` | derives [`Validate`][rustdoc-validate] |

`serde_json` gains the `preserve_order` feature under the default collection
policy so that freeform JSON objects keep their key order. With
[`--no-ordered-collections`](#ordering-and-collections) the feature is omitted.
Likewise, `validator` gains its `indexmap` feature when the generated code also
uses `indexmap`. The feature checks the length of an `IndexSet` or `IndexMap`
field, such as the `maxItems` limit on the `tags` of `NewPet`.

### Using the Crate

The manifest declares a plain `[package]` with no `[workspace]` table, so the
crate builds standalone and can also be added to an existing workspace:

```toml
[workspace]
members = ["crates/*", "petstore-api"]
```

To use the generated crate from another crate, add a path dependency to that
crate's `Cargo.toml`:

```toml
[dependencies]
petstore-api = { path = "../petstore-api" }
```

Keep the default [`--visibility public`](#visibility) for a crate other code
depends on. `crate` and `file` visibility restrict the re-exports in `lib.rs`, so
other crates can't access those re-exported items.

## Generated Names

Schema and property names become Rust identifiers. When a name isn't a valid
identifier, or would collide with something Rust or the generated code already
uses, the generator changes it as shown in the following table:

| Name in the specification | Generated name | Reason |
|---|---|---|
| A property named after a keyword, such as `type` or `match` | `r#type` | Keywords need the raw identifier prefix |
| A property named `crate`, `self`, or `super` | `crate_`, `self_`, `super_` | These keywords can't be raw identifiers |
| A property that starts with a digit, such as `2fa` | `_2fa` | Identifiers can't start with a digit |
| A schema named after a prelude item, such as `Option` or `Vec` | `OptionType` | The name would shadow the prelude |
| A schema named `Self` | `SelfType` | `Self` can't be a raw identifier |
| A schema named `S` | `SType` | Generated server handlers use `S` for your service type |
| A schema that starts with a digit, such as `123Response` | `T123Response` | Identifiers can't start with a digit |

A renamed property keeps its original name on the wire through
`#[serde(rename)]`, so the JSON stays the same. Enum variants follow the same
rules as schema names. For example, the enum value `self` becomes the variant
`SelfType`.

## Field Validation

Schemas often limit the values they accept: a string's length, a number's
range, or an array's size. The generator turns the limits it supports into
[`validator`][rustdoc-validate] attributes on the generated fields, and
`validate()` checks a value against them. The following table shows which
attribute each schema keyword produces:

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

A required string without `minLength` also gets `length(min = 1)`, so an empty
string fails validation. Strings with a `date`, `date-time`, `time`, `uuid`,
`byte`, or `binary` format don't get length or pattern checks.

The petstore's `CollarTelemetry` schema uses several of these keywords. Here's
an excerpt of the generated struct:

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

The `collarId` property is required and has a `pattern`, so `collar_id` gets
both a minimum length and a regular expression check. `batteryPercent` declares
`minimum: 0`, `maximum: 100`, and `multipleOf: 5`. The same rules apply to a
request body that isn't an object. For example, the `text/plain` body of
`setPetNotes` declares `maxLength: 2000`, so the body field of
`SetPetNotesRequest` carries `length(max = 2_000u64)`.

### Multiples

A `multipleOf` check calls a function from `oas3-gen-support`. An integer
field calls `multiple_of::<N>`, which accepts a value that divides evenly by
`N`. With `multipleOf: 5`, `batteryPercent` accepts `85` and rejects `87`. A
number field calls `multiple_of_ratio::<NUMERATOR, DENOMINATOR>`, with the
multiple written as a fraction: `multipleOf: 0.25` becomes
`multiple_of_ratio::<1, 4>`. That check applies only to `f32` and `f64` fields.
It accepts a value when the nearest multiple of the fraction, rounded to the
field's type, is the value itself. It also accepts a value whose quotient by the
fraction is within `1e-9` of a whole number, which absorbs rounding that
accumulates in arithmetic. So `0.3` counts as a multiple of `0.1` in either
type, as do `0.1 + 0.2` and a sum of one hundred `0.1` values, while `0.35`
doesn't. Where the field's type is coarser than the multiple, every value rounds
to a multiple, so an `f32` field with `multipleOf: 0.1` accepts `1_100_000.125`.
Either check reports a failure as a validation error with the code
`multiple_of`.

Every `multipleOf` adds a line such as `Validation: Must be a multiple of 5` to
the field's documentation. These multiples get only that line:

- A fractional multiple on an integer field, such as `multipleOf: 0.5`.
- A multiple that can't be written as a fraction whose numerator and
  denominator fit in a [`u32`][rustdoc-u32], such as `1e-20`.

### When Validation Runs

Generated types derive [`Validate`][rustdoc-validate] only when the API
accepts them, such as request structs and request bodies. A schema used only
in responses, such as `Pet`, keeps no validation attributes. Request builders
validate when you call `build()`, as the [Builder Pattern](./builders.md)
chapter explains. [Making a Request](./client-generation.md#making-a-request)
and [Implementing the Service Trait](./server-generation.md#implementing-the-service-trait)
cover when the client and server check a request.

## Visibility

Use `--visibility` to choose where generated items can be accessed. The
default, `public`, makes them available to other crates. Choose `crate` when
the generated API is an internal part of your application.

```text
-C, --visibility <LEVEL>
```

| Value | Modifier | Use Case |
|-------|----------|----------|
| `public` (default) | `pub` | Library distribution |
| `crate` | `pub(crate)` | Internal crate types |
| `file` | `pub(super)` | Items used only inside the generated module |

Let's compare the three values on two petstore types: the `Pet` struct and the
`ChatModelChoice` union, which has [helper methods](#helper-methods).

### Example: `--visibility public`

With public visibility, the types, fields, and methods use `pub`:

```rust
pub struct Pet {
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

pub enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

### Example: `--visibility crate`

With crate visibility, those items use `pub(crate)`:

```rust
pub(crate) struct Pet {
    pub(crate) id: uuid::Uuid,
    pub(crate) name: String,
    /* ... */
}

pub(crate) enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub(crate) fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

### Example: `--visibility file`

With file visibility, those items use `pub(super)`:

```rust
pub(super) struct Pet {
    pub(super) id: uuid::Uuid,
    pub(super) name: String,
    /* ... */
}

pub(super) enum ChatModelChoice {
    ChatModel(ChatModel),
    String(String),
}

impl ChatModelChoice {
    pub(super) fn chat10() -> Self {
        Self::ChatModel(ChatModel::Chat10)
    }
    /* ... */
}
```

In module output, `pub(super)` lets the generated files use each other's items:
`client.rs` or `server.rs` can reach the types in `types.rs`. The module's
`mod.rs` or `lib.rs` doesn't re-export anything, so the items stay inside the
generated module.

## Enum Mode

Different schema values can become the same Rust variant name. For example,
`"ACTIVE"` and `"active"` both become `Active`. Use `--enum-mode` to choose
whether to merge those values, keep them separate, or accept differences in
ASCII letter case during deserialization.

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

The default mode merges values that produce the same Rust identifier. It
uses the first value as the serialization name and adds the others as Serde
aliases for deserialization:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ACTIVE", alias = "active", alias = "Active")]
    Active,
    #[serde(rename = "PENDING")]
    Pending,
}
```

All three spellings of `"ACTIVE"` in the schema deserialize to
`Status::Active`. The lowercase string `"pending"` produces an error because
it isn't declared as a value or alias. Merging preserves the declared
spellings; it doesn't make every spelling case-insensitive.

### Example: `--enum-mode preserve`

Use `preserve` when your application needs to distinguish the declared
spellings. Each value gets its own variant, and colliding names get numeric
suffixes:

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

Now `"ACTIVE"`, `"active"`, and `"Active"` deserialize to `Status::Active`,
`Status::Active1`, and `Status::Active2`, respectively. Your code can match
those variants separately.

### Example: `--enum-mode relaxed`

Use `relaxed` when you want to accept values regardless of ASCII letter
case. The generator merges the variants and writes a [`Deserialize`][rustdoc-serde-deserialize]
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

This version accepts both `"PENDING"` and `"pending"` as `Status::Pending`,
as well as the different spellings of `"ACTIVE"`. Serialization still uses
the declared name shown in the `#[serde(rename)]` attribute.

## Enum Layout

Once you've chosen how to handle enum values, you can choose their order in
the generated source. The `--enum-layout` flag preserves the specification's
order by default or sorts variants by their Rust names.

```text
--enum-layout <LAYOUT>
```

| Value | Behavior |
|-------|----------|
| `spec` (default) | Preserve variant order from the OpenAPI document |
| `sorted` | Sort variants alphabetically by Rust variant name |

`sorted` applies to value enums (string `enum`), `oneOf`/`anyOf` union variants,
and discriminated enum variants. HTTP response (status-code) enums are unaffected
and continue to use status-code order.

Use `sorted` when you want the declarations to stay in the same order after
the specification reorders enum values. This can make changes to the generated
source easier to review.

Ordering can also affect the default value. For value enums that derive
[`Default`][rustdoc-default], the generator marks the variant matching the schema's `default`.
If no variant matches, it uses the first declared variant. Switching to
`sorted` can therefore change the fallback default.

Untagged unions, described with `oneOf` or `anyOf`, always place a variant that
wraps a plain `String` last, after any sorting. Deserialization tries an
untagged union's variants in order, and a `String` accepts every JSON string.
The petstore's `ChatModelChoice` lists `{ "type": "string" }` before a `$ref`
to the `ChatModel` enum, but its generated `ChatModel(ChatModel)` variant comes
first, so `"chat-1.0"` decodes as `ChatModel(ChatModel::Chat10)`.

Variants that wrap an array come first. The petstore's `ArrayOrSingle` lists a
`TextBlock` before an array of them, and the generated enum tries the array
first:

```rust
#[serde(untagged)]
pub enum ArrayOrSingle {
    Array(Vec<TextBlock>),
    #[default]
    TextBlock(TextBlock),
}
```

The array's move doesn't change the default: `TextBlock` keeps `#[default]`.

Untagged unions also handle defaults differently. When a variant can represent
the schema's default, the generator writes an `impl Default` that constructs
that value, checking variants in the same order deserialization does. For
example, the `image_size` property of `ContentRequest` defaults to `auto_2K`,
so its union's default returns `Self::Preset(ImageSizePreset::Auto2k)`. A
string default produces a value such as `Self::String("cheesecake".to_string())`.
Without a usable default, a union that derives `Default` uses the first variant
that can't lead back to the union, so the derived `Default` never recurses. Unit
and const variants qualify, as do variants wrapping an array, a nullable type, or
a payload that doesn't refer back to the union. The generator picks it in spec
order, or in sorted order under `--enum-layout sorted`, and moving arrays or plain
strings ahead for deserialization doesn't change it. A union with a `null` branch
records the same variant in case a struct's derived `Default` later needs the
union to derive one.

A property can also set a `default` next to its `$ref`, as OpenAPI 3.1 allows.
When the reference names an enum, a `const`, or a union, the field's default
uses that value instead of the referenced schema's own `default`. The
petstore's `ChatPreference` schema does this:

```json
"fallback": { "$ref": "#/components/schemas/ChatModel", "default": "chat-1.1" }
```

This property becomes `#[default(Some(ChatModel::Chat11))] pub fallback: Option<ChatModel>`,
along with the deserialization default described in [Field Defaults](#field-defaults).
A union's default selects its matching variant, so `"default": "slow"` on a
`oneOf` of `const` values becomes `#[default(Some(Mode::Slow))]`. The generator
ignores a `default` next to any other reference, because it can't build that
value for the referenced type. A default that matches no variant of an enum or
union, such as an object default on a referenced or inline `oneOf`, gets no
`#[default(...)]` attribute, so the field keeps its plain `Default` instead of a
placeholder that picks the first variant.

A nullable union includes a `null` branch. If it has no usable default, it gets
no `Default` implementation. Properties with that union type use [`Option<T>`][rustdoc-option],
including properties listed as required.

### Input Schema

The petstore's `Species` schema lists its values in an order that isn't
alphabetical, and it has no `default`. Its `x-enum-descriptions` extension
documents each value:

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

The default layout keeps `Dog` first, followed by the remaining values in
schema order. Because `Dog` is the first declared variant, it's also the
default:

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

The sorted layout puts `Bird` first. The serialization names and descriptions
remain attached to the same variants, but the fallback default moves to `Bird`:

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

An enum can represent numbers as well as strings. When a schema combines an
`enum` array with `type: integer` or `type: number`, the generated type reads
and writes JSON numbers. The generator does the same when a schema lists only
numbers and declares no `type`, choosing `i64` for whole numbers and `f64` when
any value has a fractional part. The generator supplies [`Serialize`][rustdoc-serde-serialize] and [`Deserialize`][rustdoc-serde-deserialize]
implementations so that a value such as `8000` stays a number during
serialization. The petstore's `Priority` schema, `{ "enum": [1, 2, 3] }`, is
an example without a `type`.

Consider the petstore's `SampleRate` schema, which lists the sample rates the
assistant accepts:

```json
"SampleRate": {
  "description": "Audio sample rate in Hz (integer-backed enum)",
  "type": "integer",
  "enum": [8000, 16000, 24000, 44100, 48000],
  "default": 24000
}
```

The generator emits one variant per value, a serializer that turns each variant
into its number, and a deserializer that maps numbers back to variants. The
schema's `default` marks `Value24000` as the [`Default`][rustdoc-default]
variant. These implementations use the [`serde::Serializer`][rustdoc-serde-serializer] and [`serde::Deserializer`][rustdoc-serde-deserializer] traits,
with [`serde::de::Error`][rustdoc-serde-de-error] for decoding errors:

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

`SampleRate::Value8000` now serializes to `8000`, and `8000` deserializes back
to `SampleRate::Value8000`. A number outside the listed values produces an
error that names the accepted values.

The schema's type and format determine which Rust number type the generated
implementation uses. Signed integers use [`i64`][rustdoc-i64], unsigned integers use [`u64`][rustdoc-u64],
and floating-point values use [`f64`][rustdoc-f64]. In each case, the serialized value is a
JSON number.

### Floating-Point Values

For a `number` enum, the generated deserializer reads an [`f64`][rustdoc-f64] and compares
its bit pattern with each allowed value. Here's an excerpt for the petstore's
`PlaybackRate`, which accepts `0.5`, `1.0`, `1.5`, and `2.0`:

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

The Rust variants don't store floating-point values: each variant identifies
one allowed number. These enums can therefore derive [`Eq`][rustdoc-eq] and [`Hash`][rustdoc-hash], and you
can use them as map keys or set members.

## Boolean and Mixed-Type Enums

An `enum` array can also list booleans, or mix strings, numbers, and booleans.
Each listed value keeps its JSON type on the wire, so a `true` in the schema is
written and read as the JSON boolean `true`, never as the string `"true"`.

The petstore's `Toggle` schema, `{ "type": "boolean", "enum": [true, false] }`,
produces `True` and `False` variants with a serializer that writes a JSON
boolean and a deserializer that reads one:

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

When the listed values mix JSON types, as in the petstore's `ResponseQuality`
schema, `"enum": ["auto", 1, 0.5, true, 1e-6, 1e21, 18446744073709552000]`, each
variant writes its own type, and the deserializer reads a
[`serde_json::Value`][rustdoc-serde-json-value] and matches it against every
listed value:

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

`ResponseQuality::Value1` reads the JSON number `1`. Query strings and path
segments hand every value over as a string, so a numeric or boolean variant also
reads a string that parses to its value: `1` selects `Value1`, `0.5` selects
`Value0_5`, and `1e-6` selects the variant for `0.000001`. Boolean text matches
exactly, or case-insensitively under
[`--enum-mode relaxed`](#example---enum-mode-relaxed), where `TRUE` selects
`True`. Listed strings are tried first, so in `["1", 1]` the string `"1"` selects
the string variant while the number `1` still selects the number variant. This
applies to JSON too: when the enum doesn't list the string `"1"`, the JSON string
`"1"` decodes to the number variant.

A declared `type` takes precedence over the listed values. With
`"type": "string"`, every value is read and written as a string.

## Discriminated Unions

A `oneOf` lists alternative schemas for one value. When it declares a
`discriminator`, one property of each object names its branch, and the
generator produces an enum that reads that property to choose a variant. The
petstore's `ContentBlock` schema uses its `type` property this way, mapping
`text` to `TextBlock`, `image` to `ImageBlock`, and so on. Here's an excerpt of
the generated enum:

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

The deserializer reads the whole object, looks up `type`, and decodes the
object as the matching branch. An object without `type` produces a
missing-field error, and an unlisted value produces an error that names it. To
serialize, the enum writes the wrapped struct, whose constant `type` field
supplies the tag, as [Constant Properties](#constant-properties) describes.

### Inferred Discriminators

Some documents leave out the `discriminator` even though each branch fixes a
property to its own constant. The petstore's `UsageCounterA` and
`UsageCounterB` schemas differ only in their `type` constants. Here's
`UsageCounterA`, trimmed of its `title` and `default` keywords:

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

`UsageCounterB` is the same apart from `"const": "counter_b"`. The items of
`UsageCounters` are an `anyOf` of these two schemas with no `discriminator`.

The generator infers a discriminator when a `oneOf` or `anyOf` meets all of
these conditions:

- It declares no `discriminator`.
- It has at least two branches other than `null`, and each is a `$ref` to an
  object schema.
- Every such branch requires the same property and sets it to a string
  `const`.
- No two branches use the same constant.

The resulting enum routes on that property, exactly as if the document had
declared it:

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

Now `{"type": "counter_b", "output_tokens": 3}` decodes as
`UsageCounterUsageCounterKind::B`. A union that misses any of the conditions,
such as one with an inline branch, stays untagged.

### Nullable Arrays of Unions

`UsageCounters` itself pairs an inline array with `null`:

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
[`Vec`][rustdoc-vec] of the item union, and a field that uses it carries the
nullability as an [`Option`][rustdoc-option], as the `iterations` field of
`ToolResultBlock` shows:

```rust
pub type UsageCounters = Vec<UsageCounterUsageCounterKind>;

pub struct ToolResultBlock {
    /* ... */
    pub iterations: Option<UsageCounters>,
    /* ... */
}
```

Any `anyOf` or `oneOf` that pairs a single inline array with `null` converts
this way, whatever the array's items are.

## Constant Unions

A `oneOf` or `anyOf` whose every branch is a `const` describes the same set of
values as an `enum` array, so the generator produces the same value enum. Each
`const` becomes a unit variant documented with its branch's `description`, and
the schema's `default` marks the matching variant. The petstore's
`ResponseKind` schema is one of these:

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
[Boolean and Mixed-Type Enums](#boolean-and-mixed-type-enums), and `null` branches
make the property optional instead of adding a variant.

A union can also mix `const` branches with branches that carry a payload, such
as an object or a string. Its `const` variants stay tagged so that each matches
its wire value, and they come first. Only the payload variants are marked
`#[serde(untagged)]`. The petstore's `UsageChoice` lists a string, a `$ref` to
the `Usage` object, and the constant `"auto"`, which is also its default:

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

Here `"auto"` deserializes to `UsageChoice::Auto` and serializes back to
`"auto"`. An object becomes `UsageChoice::Usage`, and any other string becomes
`UsageChoice::String`. A schema `default` that names a constant, like `"auto"`,
selects its variant.

## Non-Exhaustive Enums

Some APIs list the values they support today and promise to accept others
later. A specification describes that promise with an `anyOf` that pairs the
listed values with a freeform string. This section shows the single enum the
generator creates for that pattern and how your code can handle values the
list doesn't include.

Consider the petstore's schema for a model name. It pairs a freeform string
with the `ChatModel` enum, which lists `chat-1.0` and `chat-1.1`:

```json
"ChatModelId": {
  "description": "A listed chat model or any other model name",
  "anyOf": [{ "type": "string" }, { "$ref": "#/components/schemas/ChatModel" }]
}
```

The generator emits one variant per listed value, followed by an `Other`
variant that holds any other string. It marks the enum `#[non_exhaustive]`:

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

The generator copies the referenced values into the open enum. `ChatModel`
itself stays exhaustive, so a field typed `ChatModel` still rejects other
values.

Deserialization checks the listed values first, so `"chat-1.0"` becomes
`ChatModelId::Chat10`. A value outside the list, such as `"chat-2.0"`, becomes
`ChatModelId::Other("chat-2.0".to_string())` instead of producing an error.
Because `Other` is `#[serde(untagged)]`, it serializes as the bare string, so an
unlisted value you received is sent back unchanged. The [`Display`][rustdoc-display]
implementation writes the same string. You construct a listed value through its
variant, such as `ChatModelId::Chat11`, and the enum still derives
[`Eq`][rustdoc-eq] and [`Hash`][rustdoc-hash].

`ChatModelId::Other("chat-1.0".to_string())` serializes like
`ChatModelId::Chat10` but doesn't compare equal to it, so reserve `Other` for
values outside the list. When a model name comes from your own code, such as a
configuration file, convert it with the generated `From<String>` implementation
instead of building `Other` yourself:

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

`ChatModelId::from(name)` returns `ChatModelId::Chat10` for `"chat-1.0"` and
`Other` for anything else, the same result deserialization gives. The
comparison `ChatModelId::from(name) == ChatModelId::Chat10` then behaves as
you'd expect.

The [`#[non_exhaustive]`][reference-non-exhaustive] attribute matters when the
generated types live in their own crate, as with
[workspace crate output](#workspace-crate-output). A `match` in another crate
must then include a wildcard arm. When a later version of the specification
lists another value, regenerating adds a variant without breaking that `match`.
Inside the crate that defines the enum, the attribute has no effect.

A few details follow from the same rules:

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

## Field Defaults

A property's `default` supplies its value when the JSON leaves the property
out. The generator uses it in two places. A `#[default(...)]` attribute sets
the field's value in the type's [`Default`][rustdoc-default] implementation,
and a struct-level `#[serde(default)]` tells Serde to take every missing field
from that implementation during deserialization.

Let's look at the petstore's `Order` schema, which requires `petId` and gives
`quantity` and `complete` defaults. Here it is without its `shipDate` and
`status` properties:

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

The generated struct carries the defaults on its fields and a single Serde
attribute on the struct:

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

Deserializing `{"petId": "..."}` gives `quantity` the value `Some(1)` and
`complete` the value `Some(false)`, the same values `Order::default()` holds.

The attribute applies to the whole struct, so it fills required fields too.
JSON without `petId` deserializes with the nil UUID from `Order::default()`
instead of failing. A struct only gets `#[serde(default)]` when at least one of
its fields has a `default` or a `const`, so required fields in other structs
are still rejected when they're missing.

Client output also reads `null` in these structs' required fields as a
default. See [Missing and Null Fields](./client-generation.md#missing-and-null-fields).

## Constant Properties

A property with a `const`, or with an `enum` that lists a single value, has that
value as its default, so an absent property takes it like any spec default. The
optional `object` property of the petstore's `ChatPreference` becomes:

```rust
#[default(Some("chat.preference".to_string()))]
pub object: Option<String>,
```

The client keeps a different value it receives rather than replacing it, as
[Constant Properties](./client-generation.md#constant-properties) describes.

## Helper Methods

By default, the generator adds constructor helpers for supported enum
variants. For a variant that wraps a struct with a [`Default`][rustdoc-default] implementation,
a helper can accept the required values and fill in the remaining fields.
Pass `--no-helpers` if you'd prefer to construct the variants yourself.

```text
--no-helpers
```

### Default (Helpers Enabled)

For the petstore's `ContentBlock` enum, the helpers construct the nested
structs and wrap them in the matching variants:

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

Each helper takes the one field its struct requires, and `..Default::default()`
fills in the rest, including the constant `type` field. `ToolUseBlock` and
`ToolResultBlock` each require more than one field, so their variants get no
helper.

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

## OData Support

An OData API may omit metadata fields that its schema lists as required. If
you encounter this with fields such as `@odata.type`, use `--odata-support` to
make eligible fields optional in the generated types.

```text
--odata-support
```

This rule applies to names starting with `@odata.` when the parent schema has
no discriminator and isn't an intersection using `allOf`.

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

Now deserialization succeeds when either metadata field is absent, and the
corresponding Rust field is `None`. The `id` field is still required because
its name doesn't start with `@odata.`.

## Type Customization

Use `--customize` to choose a `serde_with` adapter for a generated field. An
adapter controls how Serde reads or writes a value. The generated field keeps
its Rust type, and the adapter appears in its [`#[serde_as]`][rustdoc-serde-as] attribute.

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

The date-time type uses [`chrono::Utc`][rustdoc-chrono-utc] as its time zone. For these examples,
suppose your crate defines adapters named `DateTimeAdapter` and `DateAdapter`.
Each adapter must implement [`serde_with::SerializeAs`][rustdoc-serialize-as] or
[`serde_with::DeserializeAs`][rustdoc-deserialize-as] as needed by the generated field. To use both
adapters, repeat the flag:

```bash
cargo run -- generate types -i petstore.json -o types.rs \
  -c date_time=crate::DateTimeAdapter \
  -c date=crate::DateAdapter
```

### Default (No Customization)

The petstore's `CollarTelemetry` schema has a required `recordedAt` date-time
and an optional `recordedOn` date. Without an override, the fields use their
normal Serde implementations:

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

If we pass only `-c date_time=crate::DateTimeAdapter`, the generator adds an
adapter to `recorded_at`:

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

Notice that `recorded_at` is still a [`chrono::DateTime<chrono::Utc>`][rustdoc-chrono-datetime]. The
`DateTimeAdapter` controls its conversion during serialization and
deserialization. The `recorded_on` field is unchanged because we haven't
supplied a `date` override in this example. The generator also adds
`#[serde_with::serde_as]` to any struct that uses an adapter.

### Handling Optional and Array Fields

The generator wraps the adapter in [`Option`][rustdoc-option] and [`Vec`][rustdoc-vec] to match optional and
array fields. For example, the same date-time override gives the optional
`homeVisit` property of `AdoptionRequest` this attribute:

```rust
pub struct AdoptionRequest {
    /* ... */
    #[serde_as(as = "Option<crate::DateTimeAdapter>")]
    #[serde(rename = "homeVisit")]
    pub home_visit: Option<chrono::DateTime<chrono::Utc>>,
}
```

An array of date-times would get `Vec<crate::DateTimeAdapter>`, and an
optional array `Option<Vec<crate::DateTimeAdapter>>`. Each attribute follows
the shape of its field. You supply the adapter for one date-time value; the
generator adds the surrounding container adapters.

## Operation Filtering

You may only need a few operations from a large API. Use `--only` to select
those operations, or `--exclude` to leave specific operations out. You can use
one of these flags at a time.

```text
--only <id1,id2,...>
--exclude <id1,id2,...>
```

### `--only`

Pass the normalized `snake_case` operation IDs, separated by commas. Filtering
uses these IDs before any `--fn-name` overrides. For example, the petstore's
`listPets` and `createPet` operations use `list_pets` and `create_pet` in the
filter:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --only list_pets,create_pet
```

The generated client keeps `list_pets()` and `create_pet()` and omits the
other operations. With `server-mod`, the same selection keeps those two trait
methods and their routes. Types needed by the selected methods are still
included, as we'll see in [Schema Dependency Resolution](#schema-dependency-resolution).

### `--exclude`

Use `--exclude` when you want most of an API but don't need a particular
operation. For example, to leave out the image upload operation, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --exclude upload_pet_image
```

The client keeps the petstore's other operations, including `list_pets()`
and `create_pet()`, and omits `upload_pet_image()`. In `server-mod` mode, the
flag omits that operation's trait method and route.

A callback follows the operation that declares it. The petstore's `adopt_pet`
operation declares the `statusChanged` callback, which `server-mod` generates as
`adopt_pet_status_changed`. In `server-mod`, `--only adopt_pet` keeps that
callback's route and trait method, and `--exclude adopt_pet` removes them.

You can also name a callback by its own normalized `snake_case` ID.
`--only adopt_pet_status_changed` keeps only the callback, and
`--exclude adopt_pet_status_changed` removes only the callback while
`adopt_pet` stays.

### Schema Dependency Resolution

Filtering an operation also filters the types needed to use it. The generator
follows dependencies through the schemas so that a selected operation has the
types it needs:

1. It collects schemas used by the selected operations' parameters, request
   bodies, and responses.
2. It follows references from those schemas to other schemas.
3. It generates the resulting set of types.

For example, the petstore's `listPets` returns `Pets`, an array of `Pet`
values, and an `Error` for its `default` response. Selecting only `list_pets`
also generates the schemas `Pet` refers to: `Species`, `PetStatus`, the
`Health` union with its `Allergies` and `Diet` branches, `PetVaccinations`, and
`VaccineRecord`. The operation doesn't refer to any of them directly.

## Function Name Overrides

Use `--fn-name` to choose the generated client or server method name for an
operation. The key is its `operationId` from the specification; the generator
also accepts the normalized operation ID shown by `oas3-gen list operations`.

```text
--fn-name <ID=NAME>
```

The generator converts your method name to `snake_case` and uses it to derive
the request type's name. For example, renaming `listPets` to `fetch_all_pets`
also produces `FetchAllPetsRequest`. Repeat the flag to rename more than one
operation:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ \
  --fn-name listPets=fetch_all_pets \
  --fn-name showPetById=get_pet
```

The override changes both names together:

| Operation ID | Method Name | Request Type |
|---|---|---|
| `listPets` | `fetch_all_pets` | `FetchAllPetsRequest` |
| `showPetById` | `get_pet` | `GetPetRequest` |

Use the new names when calling the client or implementing the server trait.

## API Name Override

Use `--api-name` to name the client struct in `client` or `client-mod` mode,
or the server trait in `server-mod` mode. The generator converts the supplied
name to `PascalCase`.

```text
--api-name <NAME>
```

By default, the client name comes from the specification's `info.title`.
For example, the petstore's title, `"Petstore"`, becomes `PetstoreClient`; an
empty title produces `ApiClient`. The default server trait name is `ApiServer`.
To choose `PetStoreClient` for the client, run:

```bash
cargo run -- generate client-mod -i petstore.json -o output/ --api-name PetStoreClient
```

The client struct and its implementation now use `PetStoreClient`. In server
mode, choose a trait name with the same flag:

```bash
cargo run -- generate server-mod -i petstore.json -o output/ --api-name PetStoreApi
```

The server trait is named `PetStoreApi`, and the generated router uses that
name in its trait bound. Changing the API name doesn't rename the operations;
use [`--fn-name`](#function-name-overrides) for those.

## Schema Filtering

By default, the generator includes schemas used by the selected operations
and the schemas they depend on. Pass `--all-schemas` when you also need types
that no operation references.

```text
--all-schemas
```

### Default (Reachability Filtering)

This example uses a different document. Suppose a specification defines
`Pet`, `Category`, `Store`, and `Inventory`, but its operations only use `Pet`
and `Category`. Generate types with the default options:

```bash
cargo run -- generate types -i spec.json -o types.rs
```

The output contains `Pet` and `Category`. The generator skips `Store` and
`Inventory` because no selected operation needs them. With `--verbose`, it
reports how many of these orphaned schemas it skipped.

### With `--all-schemas`

To include those unused schemas too, add the flag:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
```

The output now contains all four types: `Pet`, `Category`, `Store`, and
`Inventory`.

### Combining with Operation Filtering

You can't combine `--all-schemas` with `--only` or `--exclude` in one command.
If you need both a complete set of types and a filtered client, generate two
outputs:

```bash
cargo run -- generate types -i spec.json -o types.rs --all-schemas
cargo run -- generate client -i spec.json -o client.rs --only list_pets
```

The first file contains all schemas. The second contains the filtered client
and its own required types; these commands don't make the client import types
from the first file.

## Header Emission

Header name constants use [`http::HeaderName`][rustdoc-http-header-name], so you can refer to headers
without repeating their string names. By default, the generator creates constants for request parameters and
[response headers](#response-headers) used by the selected operations. Use
`--all-headers` to include header parameters from `components/parameters` even
when no operation references them.

Headers that the `http` crate already names are the exception. The
[`http::header`][rustdoc-http-header-constants] module declares constants for
standard headers such as `Content-Type`, `Location`, and `Authorization`. The
generator doesn't declare its own constant for any of them; generated code
refers to `http`'s constant, such as `http::header::LOCATION`, instead.

OpenAPI says to ignore header parameters named `Accept`, `Content-Type`, or
`Authorization`, so the generator leaves them out of request types. The
specification describes those headers elsewhere: media types cover `Accept` and
`Content-Type`, and security schemes cover `Authorization`. For example, the
petstore's `listAuditEvents` declares a required `Authorization` header
parameter, yet its request struct has no header field. To send credentials in
`Authorization`, declare a [security scheme](#security-schemes), as that
operation also does.

```text
--all-headers
```

### Default (Operation-Referenced Headers Only)

The petstore's `showPetById` operation uses an `x-api-version` header. The
document also declares an `x-debug-trace` header parameter in
`components/parameters` that no operation uses. Start with the default
options:

```bash
cargo run -- generate types -i petstore.json -o types.rs
```

Among the other header constants, the output contains one for the header the
operation uses:

```rust
pub const X_API_VERSION: http::HeaderName = http::HeaderName::from_static(
    "x-api-version",
);
```

`x-debug-trace` isn't emitted because no operation uses it.

### With `--all-headers`

Now include the unused component header:

```bash
cargo run -- generate types -i petstore.json -o types.rs --all-headers
```

The output gains a constant for it:

```rust
pub const X_DEBUG_TRACE: http::HeaderName = http::HeaderName::from_static(
    "x-debug-trace",
);
```

## Builder Generation

Use `--enable-builders` when you want to construct generated values through
setter methods. The generator adds [`bon::Builder`][rustdoc-bon-builder] derives to schema structs
and `#[builder]` constructors to request structs.

```text
--enable-builders
```

Builders are disabled by default. The [Builder Pattern](./builders.md) chapter
walks through using them and explains how request builders run validation.

### Default (Builders Disabled)

Without the flag, generated structs have no `bon` attributes. This excerpt
from the petstore client shows a schema type and a request type:

```rust
/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

/// Get a pet
#[derive(Debug, Clone, validator::Validate, oas3_gen_support::Default)]
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    #[validate(nested)]
    pub header: ShowPetByIdRequestHeader,
}
```

### With `--enable-builders`

With the flag, the schema gets a derive and the request gets a constructor
annotated for `bon`:

```rust
/// A pet in the store's care.
#[derive(Debug, Clone, PartialEq, Deserialize, oas3_gen_support::Default, bon::Builder)]
pub struct Pet {
    /// Server-assigned identifier
    pub id: uuid::Uuid,
    pub name: String,
    /* ... */
}

/// Get a pet
#[derive(Debug, Clone, validator::Validate, oas3_gen_support::Default)]
pub struct ShowPetByIdRequest {
    pub path: ShowPetByIdRequestPath,
    #[validate(nested)]
    pub header: ShowPetByIdRequestHeader,
}

#[bon::bon]
impl ShowPetByIdRequest {
    /// Create a new request with the given parameters.
    #[builder]
    pub fn new(
        pet_id: uuid::Uuid,
        if_none_match: Option<String>,
        x_api_version: String,
    ) -> anyhow::Result<Self> {
        /* ... */
    }
}
```

The constructor takes each parameter as a flat argument, so the builder has a
setter for `pet_id`, `if_none_match`, and `x_api_version`. It assembles the
nested `path` and `header` structs and validates the request before returning
it.

## Ordering and Collections

By default, the generator preserves declaration order from the OpenAPI
document for schemas, fields, variants, operations, and header constants.
You can change enum ordering separately with
[`--enum-layout`](#enum-layout).

The generator also uses collection types that preserve insertion order at
runtime. Maps described by `additionalProperties` use
[`indexmap::IndexMap<String, T>`][rustdoc-indexmap-map], and arrays with `uniqueItems: true` use
[`indexmap::IndexSet<T>`][rustdoc-indexmap-set]. For example, the `tags` of a petstore `Pet` are
`uniqueItems`, so the field is `Option<indexmap::IndexSet<String>>`, and its
`vaccinations` map is `Option<indexmap::IndexMap<String, Vec<VaccineRecord>>>`.
If you include these types in an existing crate, add `indexmap` with its
`serde` feature to your `Cargo.toml`. A generated workspace manifest includes
that dependency when needed.

A `uniqueItems` field also carries a `serde_with::SetPreventDuplicates`
adapter, so a JSON array with a repeated item fails to deserialize:

```rust
#[serde_as(as = "Option<serde_with::SetPreventDuplicates<_>>")]
pub tags: Option<indexmap::IndexSet<String>>,
```

Use this flag to choose standard library collections instead:

```text
--no-ordered-collections
```

Set `--no-ordered-collections` to opt out of the `indexmap` runtime types. Map
schemas then resolve to [`std::collections::HashMap<String, T>`][rustdoc-hashmap], and
`uniqueItems` arrays resolve to [`Vec<T>`][rustdoc-vec], the same type as a normal array, and
accept repeated items. Use this flag when your
code cannot depend on `indexmap`, or when you do not need JSON key and element
order to survive a deserialize-then-serialize round trip.

This flag does not change the declaration order of items in the generated
source, such as struct fields, enum variants, and operation methods.

## Documentation Formatting

Descriptions and summaries in the OpenAPI document become documentation
comments in the generated code. Use `--doc-format` to format longer comments
with the external [mdformat](https://github.com/executablebooks/mdformat) tool.
The generator sends text longer than 100 bytes to `mdformat` with a line-wrap
setting of 100 characters.

```text
--doc-format
```

Install `mdformat` and make sure it's available on your `PATH` before using
the flag:

```bash
pip install mdformat
```

### Default (Formatting Disabled)

Without the flag, the generator preserves documentation text while converting
escaped newlines to line breaks. A long description can therefore remain on
one line, as the petstore's `CollarTelemetry` description does:

```rust
/// A batch of smart-collar readings, written with JSON Schema 2020-12 keywords. Keywords without a Rust equivalent are accepted and ignored.
pub struct CollarTelemetry {
    /* ... */
}
```

### With `--doc-format`

```bash
cargo run -- generate types -i petstore.json -o types.rs --doc-format
```

The same description is wrapped into multiple lines:

```rust
/// A batch of smart-collar readings, written with JSON Schema 2020-12 keywords. Keywords without a Rust
/// equivalent are accepted and ignored.
pub struct CollarTelemetry {
    /* ... */
}
```

## Known Limitations

Some parts of an OpenAPI document have no exact Rust equivalent. The generator
accepts the following schemas and parameters, but the generated code doesn't
enforce everything they say. Each item names the petstore schema or operation
that shows it:

- With `--no-ordered-collections`, an array with `uniqueItems: true`, such as
  the `tags` of `Pet`, is a `Vec` and accepts repeated items. See
  [Ordering and Collections](#ordering-and-collections).
- An optional field that also allows `null` is an `Option<T>`, so JSON `null`
  and a missing property both decode as `None`, and `None` is left out when
  serializing. A client therefore can't send `null` to clear a field with a
  JSON merge patch, such as the `nickname` in the `PetPatch` body of
  `updatePet`. Likewise, a field without a type, such as `anyValue` in
  `CollarTelemetry`, decodes `null` as `None`.
- The generator ignores `allowReserved` on query parameters. The client
  percent-encodes reserved characters, such as the `/` in a `q` value for
  `searchPets`.
- A query parameter's value must be a scalar or an array of scalars. A JSON
  object or an array of arrays in an untyped parameter, such as `filter` on
  `searchPets`, makes the client method return an error before it sends the
  request.
- An `anyOf` whose branches each require a different property isn't enforced.
  The `PetCarrier` schema requires either `wheels` or `straps`, but both become
  optional fields, so a carrier with neither decodes successfully.
- A multipart part name can't contain `"`, `\`, a carriage return, a line
  feed, or another control character. The generator warns about quotes and line
  breaks, as described in [Generation Warnings](#generation-warnings).
- A `multipleOf` that the generator can't check exactly only appears in the
  field's documentation. See [Multiples](#multiples).

## Flag Summary

This table collects the options used across the generation chapters. Defaults
apply when you omit the corresponding option. Run `oas3-gen generate --help`
to see the command-line help, including input, output, and terminal options.

| Flag | Default | Description |
|------|---------|-------------|
| `mode` | `types` | Generation mode: `types`, `client`, `client-mod`, `server-mod` |
| `-w, --workspace` | `false` | Emit a workspace-compatible `Cargo.toml` with sources in `src/`; requires `client-mod` or `server-mod` |
| `--module-version` | `0.0.0` | Version written to the generated `Cargo.toml` `[package]` table; requires `--workspace` |
| `-C, --visibility` | `public` | Item visibility: `public`, `crate`, `file` |
| `--enum-mode` | `merge` | Enum duplicate handling: `merge`, `preserve`, `relaxed` |
| `--enum-layout` | `spec` | Variant ordering: `spec`, `sorted` |
| `--no-helpers` | `false` | Disable enum constructor helpers |
| `--odata-support` | `false` | Make `@odata.*` fields optional |
| `-c, --customize` | *(none)* | Custom `serde_with` adapter; repeatable |
| `--fn-name` | *(none)* | Custom function name per operation (`ID=NAME`); repeatable |
| `--api-name` | *(none)* | Name for the generated client struct or server trait |
| `--all-headers` | `false` | Emit header constants for all component-level headers |
| `--enable-builders` | `false` | Enable `bon` builder derives and methods |
| `--no-ordered-collections` | `false` | Emit [`HashMap`][rustdoc-hashmap]/[`Vec`][rustdoc-vec] instead of `indexmap` collection types |
| `--doc-format` | `false` | Format documentation comments with `mdformat` |
| `--only` | *(none)* | Include only specified operations |
| `--exclude` | *(none)* | Exclude specified operations |
| `--all-schemas` | `false` | Generate all schemas regardless of usage |

To work through constructing the generated values, continue to the
[Builder Pattern](./builders.md) chapter.

[reference-deprecated]: https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-deprecated-attribute
[reference-non-exhaustive]: https://doc.rust-lang.org/reference/attributes/type_system.html#the-non_exhaustive-attribute
[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-bon-builder]: https://docs.rs/bon/3.10.1/bon/derive.Builder.html
[rustdoc-chrono-date]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveDate.html
[rustdoc-chrono-datetime]: https://docs.rs/chrono/0.4.45/chrono/struct.DateTime.html
[rustdoc-chrono-time]: https://docs.rs/chrono/0.4.45/chrono/struct.NaiveTime.html
[rustdoc-chrono-utc]: https://docs.rs/chrono/0.4.45/chrono/struct.Utc.html
[rustdoc-cookie]: https://docs.rs/cookie/0.18.2/cookie/
[rustdoc-default]: https://doc.rust-lang.org/std/default/trait.Default.html
[rustdoc-deserialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.DeserializeAs.html
[rustdoc-display]: https://doc.rust-lang.org/std/fmt/trait.Display.html
[rustdoc-duration]: https://doc.rust-lang.org/std/time/struct.Duration.html
[rustdoc-eq]: https://doc.rust-lang.org/std/cmp/trait.Eq.html
[rustdoc-expose-secret]: https://docs.rs/secrecy/0.10.3/secrecy/trait.ExposeSecret.html
[rustdoc-f64]: https://doc.rust-lang.org/std/primitive.f64.html
[rustdoc-from-str]: https://doc.rust-lang.org/std/str/trait.FromStr.html
[rustdoc-futures-boxstream]: https://docs.rs/futures/0.3.34/futures/stream/type.BoxStream.html
[rustdoc-hash]: https://doc.rust-lang.org/std/hash/trait.Hash.html
[rustdoc-hashmap]: https://doc.rust-lang.org/std/collections/struct.HashMap.html
[rustdoc-http-header-constants]: https://docs.rs/http/1.5.0/http/header/index.html#constants
[rustdoc-http-header-map]: https://docs.rs/http/1.5.0/http/header/struct.HeaderMap.html
[rustdoc-http-header-name]: https://docs.rs/http/1.5.0/http/header/struct.HeaderName.html
[rustdoc-http-status]: https://docs.rs/http/1.5.0/http/status/struct.StatusCode.html
[rustdoc-i64]: https://doc.rust-lang.org/std/primitive.i64.html
[rustdoc-indexmap-map]: https://docs.rs/indexmap/2.14.2/indexmap/map/struct.IndexMap.html
[rustdoc-indexmap-set]: https://docs.rs/indexmap/2.14.2/indexmap/set/struct.IndexSet.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-partial-eq]: https://doc.rust-lang.org/std/cmp/trait.PartialEq.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-serde-as]: https://docs.rs/serde_with/3.24.0/serde_with/attr.serde_as.html
[rustdoc-serde-de-error]: https://docs.rs/serde/1.0.229/serde/de/trait.Error.html
[rustdoc-serde-deserialize]: https://docs.rs/serde/1.0.229/serde/trait.Deserialize.html
[rustdoc-serde-deserializer]: https://docs.rs/serde/1.0.229/serde/trait.Deserializer.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-serde-serializer]: https://docs.rs/serde/1.0.229/serde/trait.Serializer.html
[rustdoc-serialize-as]: https://docs.rs/serde_with/3.24.0/serde_with/trait.SerializeAs.html
[rustdoc-string]: https://doc.rust-lang.org/std/string/struct.String.html
[rustdoc-u32]: https://doc.rust-lang.org/std/primitive.u32.html
[rustdoc-u64]: https://doc.rust-lang.org/std/primitive.u64.html
[rustdoc-unit]: https://doc.rust-lang.org/std/primitive.unit.html
[rustdoc-uuid]: https://docs.rs/uuid/1.26.1/uuid/struct.Uuid.html
[rustdoc-validate]: https://docs.rs/validator/0.21.0/validator/trait.Validate.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html
