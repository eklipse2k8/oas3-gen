# HTTP Requests and Responses

An operation connects an HTTP method and path to parameters, a request body, and
possible responses. This chapter explains how those declarations become Rust types,
including credentials, headers, and multipart fields.

The examples use `fixtures/petstore.json` . Rust excerpts omit unrelated fields and
attributes. For application code that uses these types, see
[Client Generation](./client-generation.md) or
[Server Generation](./server-generation.md) .

- [Operations and Parameters](#operations-and-parameters)
- [Security Schemes](#security-schemes)
- [Responses](#responses)
- [Multipart Form Bodies](#multipart-form-bodies)

## Operations and Parameters

Each operation under `paths` becomes a client method, a server trait method, and a
server route, named after its `operationId` in `snake_case` . The operation's parameters
and request body travel together in a request struct named after it. For example,
`showPetById` becomes `show_pet_by_id()` , which takes a `ShowPetByIdRequest` whose
`path` and `header` fields hold its path and header parameters.

Only server output includes the operations under `webhooks` . The petstore's
`petAdopted` webhook becomes the trait method `pet_adopted()` and the route
`/webhooks/petAdopted` . The client doesn't get a method for it.



### Path Item References

OpenAPI 3.1 lets an entry in `paths` or `webhooks` reference a reusable Path Item in
`components.pathItems` instead of declaring its operations in place. The petstore
declares the vaccination endpoints this way:

```json
"/pets/{petId}/vaccinations": { "$ref": "#/components/pathItems/PetVaccinations" }
```

The `PetVaccinations` component declares the `petId` parameter, a `get` operation named
`listVaccinations` , and a `post` operation named `recordVaccination` . The generator
follows the reference and generates those operations as though they were written under
`paths` . The client gets `list_vaccinations()` and `record_vaccination()` methods, and
the server gets both trait methods and their route. The `inventoryLow` webhook
references the `InventoryLowWebhook` component in the same way and becomes the server's
`inventory_low()` method.

A Path Item component can itself reference another component. The generator follows the
chain, and fields written next to a `$ref` , such as a `summary` , take precedence over
the fields of the component it names. The generator follows only
`#/components/pathItems/` references within the same document. A missing component or a
reference cycle produces no operations for that entry.

### Query Parameters

OpenAPI's default serialization for query parameters is the `form` style with
`explode: true` . For an array, that means one `name=value` pair per item, with the name
repeated. The petstore's `searchPets` operation declares its `tags` filter without a
`style` , so it uses that default:

```json
{
  "name": "tags",
  "in": "query",
  "description": "Pets carrying any of these tags, one `tags=` pair per tag",
  "schema": { "type": "array", "items": { "type": "string" } }
}
```

The generated query struct holds the array as a [`Vec`][rustdoc-vec] :

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

When `tags` holds `calm` and `shy` , the query carries `tags=calm&tags=shy` .
[Query Arrays](./client-generation.md#query-arrays) shows how the client sends these
values, and [Connecting the Router](./server-generation.md#connecting-the-router) shows
how the server reads them.

With `explode: false` , the items share a single pair. They're joined by a comma for
`form` , a space for `spaceDelimited` , or `|` for `pipeDelimited` , and the generator
adds a `serde_with` adapter that joins and splits them. The petstore's `listPets`
declares its `tags` filter with `style: form` and `explode: false` :

```rust
pub struct ListPetsRequestQuery {
    /* ... */
    /// Only return pets that carry every listed tag
    #[serde_as(as = "Option<oas3_gen_support::StringWithCommaSeparator>")]
    pub tags: Option<Vec<String>>,
}
```

The same two tags now travel as one `tags` pair whose value is `calm,shy` . A tag that
contains a comma arrives as two tags.

### Cookie Parameters

A parameter declared with `in: cookie` travels in the `Cookie` header. The request
struct holds an operation's cookie parameters in a nested struct in its `cookie` field.
The petstore's `getPetCard` operation declares two optional cookies, `cardTheme` and
`recentlyViewed` . This excerpt omits the structs' attributes:

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

Cookie parameters use OpenAPI's `form` style. An array's items are joined with commas
into one value, and each `name=value` pair is percent-encoded with the
[`cookie`][rustdoc-cookie] crate. [Cookie Parameters](./client-generation.md#cookie-parameters)
shows how the client sends them, and
[Receiving Cookie Parameters](./server-generation.md#receiving-cookie-parameters) shows
how the server reads them.

A cookie that carries a credential belongs in an `apiKey` security scheme with
`in: cookie` , as the petstore's `SessionCookie` does. See
[Security Schemes](#security-schemes) .

### Deprecated Operations

An operation marked `deprecated: true` is still generated. Its client method carries the
[`#[deprecated]`][reference-deprecated] attribute, as [Deprecated Operations](./client-generation.md#deprecated-operations)
shows. A deprecated schema property, such as the `tag` property of `Pet` , gets
`#[deprecated]` on its field in both outputs.

## Security Schemes

OpenAPI describes authentication in two places. A *security scheme* under
`components.securitySchemes` describes a credential and how it travels in a request. A
*security requirement* in `security` names the schemes an operation accepts. The
generator supports `apiKey` schemes in headers, query parameters, and cookies, and
`http` schemes that use bearer tokens.

Let's start with a header key. This excerpt from the petstore declares a scheme named
`ApiKeyAuth` and uses it as the default requirement:

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

The scheme name, `ApiKeyAuth` , identifies the credential in generated Rust names. The
`name` value, `X-Api-Key` , identifies the HTTP header. Operations inherit the top-level
`security` unless they declare their own requirements.

A bearer scheme is an `http` scheme whose `scheme` value is `bearer` . It has no `name`
, because the token always travels in the `Authorization` header as `Bearer <token>` .
The petstore declares one named `BearerAuth` :

```json
{
  "components": {
    "securitySchemes": {
      "BearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
    }
  }
}
```

The generator matches `bearer` in any case, as HTTP does. A `bearerFormat` value, such
as `JWT` , only documents the token, so it doesn't change the generated code. A header
parameter named `Authorization` doesn't produce credentials; OpenAPI says to ignore it,
as [Header Emission](./generation-options.md#header-emission) explains.

### Requirements

The entries in the `security` array are alternatives: a request needs to satisfy one
entry. Within an entry, all the named schemes are required. These petstore operations,
most of them under `/staff/keys` , show how the grouping changes the requirement:

| Operation's `security` | Requirement | Petstore operation |
|---|---|---|
| `[{"ApiKeyAuth": []}]`, inherited from the top level | Supply the header key. | `listApiKeys` |
| `[{"QueryKey": []}, {"SessionCookie": []}]` | Supply a query key or a session cookie. | `createApiKey` |
| `[{"ApiKeyAuth": [], "SessionCookie": []}]` | Supply both the header key and the session cookie. | `getApiKey` |
| `[{"BearerAuth": []}, {"ApiKeyAuth": []}]` | Supply a bearer token or the header key. | `revokeApiKey` |
| `[{}, {"ApiKeyAuth": []}]` | Allow anonymous access, or accept the header key. | `listPets` |
| `[]` or `[{}]` | Allow anonymous access, replacing any top-level requirement. | `getHealth` |

`QueryKey` is an `apiKey` scheme read from the `api_key` query parameter, and
`SessionCookie` is one read from the `session` cookie. `BearerAuth` is the bearer scheme
shown earlier.

The server represents a credential as required when every alternative needs it.
Otherwise, the field is optional, and the extractor checks that the supplied credentials
complete at least one alternative. These checks establish that credentials are present.
Your service still needs to decide whether their values authorize the request.

Other schemes, such as HTTP basic authentication and OAuth 2.0, don't produce generated
credentials. The generator reports a warning for each referenced scheme it can't handle.
If an operation has an alternative containing one of these schemes, all its credential
fields become optional, including those in that same alternative. The generated
extractor can't enforce the full requirement, so your application must handle
authorization for that operation. In the petstore, `replaceApiKey` accepts `BasicAuth`
or `ApiKeyAuth` , and `adoptPet` accepts OAuth 2.0, OpenID Connect, or `ApiKeyAuth` .
Both operations receive the header key as an optional field.

### Keeping Keys Out of Logs

The server stores API keys and bearer tokens as
[`secrecy::SecretString`][rustdoc-secret-string] . The client uses the same type for
query keys and bearer tokens. `SecretString` prints `[REDACTED]` in debug output and
clears its stored value when dropped. It implements neither [`Display`][rustdoc-display]
, [`Serialize`][rustdoc-serde-serialize] , nor [`PartialEq`][rustdoc-partial-eq] .

Client header keys use sensitive [`http::HeaderValue`][rustdoc-http-header-value]
values. They print as `Sensitive` , but their values aren't cleared when dropped.

To read a `SecretString` , call `expose_secret()` with
[`secrecy::ExposeSecret`][rustdoc-expose-secret] in scope; the [server example](./server-generation.md#reading-and-checking-keys)
shows how. Once exposed, the returned string is ordinary text, so keep it out of log
output.

[Sending Credentials](./client-generation.md#sending-credentials) shows how the client
stores and sends each scheme, and
[Receiving Credentials](./server-generation.md#receiving-credentials) shows the server's
credential types and rejections.

## Responses

Generated operations share a response enum named `ApiResponse` . Its variants represent
the status codes declared by the selected operations. When both success and failure
payloads are needed, the enum has the form `ApiResponse<Value, Failure>` . In module
output, you'll find it in `types.rs` .

The generator also adds an `Other` variant for responses that the document doesn't
declare. Its shape depends on the output. In client output, `Other` holds a response the
client won't decode, which it returns unread. In server output, it lets a service send a
status that the operation doesn't declare. The following table shows the possible
variant shapes:

| Variant shape | When |
|---|---|
| `Ok(Value)`, `Created(Value)`, ... | A 2xx status that carries a body in at least one operation |
| `NotFound(Failure)`, `Conflict(Failure)`, ... | Any other declared status that carries a body |
| `NoContent`, `NotModified`, ... | A status that has no body, and no [headers](#response-headers) in its class, in any operation |
| `ClientError(http::StatusCode, Failure)` | A range such as `4XX`; the concrete status travels with the body |
| `Unknown(http::StatusCode, Failure)` | The spec's `default` response, when it declares a body |
| `Other(Box<reqwest::Response>)` | Client output: a response with a status the operation doesn't declare, or a declared status under a media type it doesn't declare |
| `Other(http::StatusCode, Vec<u8>)` | Server output: a status the operation doesn't declare, when it has no `default` response |

The variants that carry a status use [`http::StatusCode`][rustdoc-http-status] . The
client's `Other` holds the unread [`reqwest::Response`][rustdoc-reqwest-response] , so
you can still read its status, headers, and body. Because that type isn't
[`Clone`][rustdoc-clone] , the client's response enum derives only
[`Debug`][rustdoc-debug] . The server's `Other` holds its raw body in a
[`Vec<u8>`][rustdoc-vec] .

Each operation supplies its own types for `Value` and `Failure` . We'll call these the
two *response classes*: `Value` covers 2xx statuses, and `Failure` covers the remaining
statuses. Before accounting for headers, the body types determine each parameter as
follows:

| Bodies declared in the class | Parameter |
|---|---|
| None | [`()`][rustdoc-unit] |
| One type | That type |
| One type plus a status that carries a payload but no body | [`Option<Type>`][rustdoc-option] |
| Several types | An untagged payload union such as `ErrorOrProblem` |
| Several types plus a status that carries a payload but no body | `Option<Union>` |

A payload union is an enum that can hold one of several body types. For example,
`showPetById` answers `404` with a `Problem` and its `4XX` and `default` responses with
an `Error` , so its `Failure` is `ErrorOrProblem` . Operations that declare the same set
of body types share that union definition. If no status uses one of the response enum's
type parameters, the generator omits that parameter. If a schema already uses the name
`ApiResponse` , the generator names the response enum `ApiResponseType` .

### Response Headers

Some operations return headers as well as a body. When any status in a response class
declares headers, the class uses `WithHeaders<Headers, Body>` as its parameter.
`Headers` is a generated struct with one field for each header declared across that
class. `Body` is the type chosen using the table above.

The wrapper keeps the headers and body together. The generator adds it beside the
response enum only when at least one response declares headers. If a schema already uses
the name `WithHeaders` , the generated name gets a numeric suffix:

```rust
/// A response body together with the headers its response declares.
#[derive(Debug, Clone)]
pub struct WithHeaders<Headers, Body> {
    pub headers: Headers,
    pub body: Body,
}
```

A header field is optional unless every status in its class marks it as required. If an
optional header is missing or can't be parsed, its value is `None` . For a list of
values, one invalid element makes the whole list `None` . A missing or invalid required
response header causes `parse_response` to return an error.

Operations with the same header fields share a header struct. The generator names that
struct after its headers, as in `XNextHeaders` or `ETagAndXCacheHeaders` , and adds a
numeric suffix if the name is already in use. A status without a body can still carry
headers: its variant then holds a `WithHeaders` value whose body is [`()`][rustdoc-unit]
or an [`Option`][rustdoc-option] .

The generator ignores `Content-Type` entries in response header definitions. A header
without a schema uses [`String`][rustdoc-string] . Enums used in headers implement
[`FromStr`][rustdoc-from-str] for both client and server output, and response headers
receive name constants in the same way as request headers.

For example, generating the whole petstore client produces this response enum and, for
the pagination headers of `listPets` , this header struct:

```rust
/// Response enum shared by every operation.
///
/// `Value` is the operation's success body type and `Failure` its error body type.
/// A status code is a unit variant when no operation gives it a body.
#[derive(Debug)]
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
    ///Response the operation does not declare, by status or media type; carries it unread.
    Other(Box<reqwest::Response>),
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

A status is a unit variant only when no operation declares a body or headers for it. No
petstore operation returns a `204` body, but `describePets` declares an `Allow` header
on its `204` response, so `NoContent` carries a `Value` . `NotModified` has neither, so
it's a unit variant.

The `listPets` operation returns
`ApiResponse<WithHeaders<XNextAndXTotalCountAndXRequestIdHeaders, Pets>, Error>` . The
client decodes that value from an HTTP response, and the server constructs it to send a
response. See [Reading Responses](./client-generation.md#reading-responses) and
[Returning Responses](./server-generation.md#returning-responses) for examples.

### Response Media Types

Each status declares its body under a media type, and that media type decides the body's
Rust type:

| Media type | Body type |
|---|---|
| JSON, including `+json` types such as `application/problem+json` | The type generated from the schema |
| `text/*`, such as `text/plain` | [`String`][rustdoc-string] |
| A binary type, such as `image/png` or `application/octet-stream` | [`Vec<u8>`][rustdoc-vec] |
| `text/event-stream` | A stream of events, described below |

For a text response, the client keeps string and byte bodies unchanged. It parses other
types through `FromStr` when they have a text representation; the server uses `Display`
to write those values. Objects and other types without a text representation are encoded
and decoded as JSON.

For example, `getPetCard` returns its kennel card as `text/plain` , so it returns
`ApiResponse<String, Problem>` . `getPetPhoto` returns `image/png` and produces
`ApiResponse<Vec<u8>, Problem>` , or `ApiResponse<reqwest::Body, Problem>` in a client
generated with [`--raw-body`](./generation-options.md#raw-response-bodies). Both declare
their `404` body as `application/problem+json` , which decodes into the `Problem` struct.

The client uses the status and `Content-Type` to choose a decoder. It ignores media type
parameters such as `charset` when matching. These rules determine whether a response
matches the declaration:

- A JSON or XML media type accepts any type of that syntax, so a status that
  declares `application/json` also reads `application/problem+json`.
- Any other media type accepts itself, and a range such as `image/*` accepts
  each of its subtypes.
- Wildcards also accept the concrete media type a generated server chooses:
  a JSON type for JSON data, `application/octet-stream` for bytes, or
  `text/plain` for text.
- A binary declaration can accept JSON when its body requires JSON decoding,
  as with an error object declared as `application/octet-stream`. This
  fallback applies only if the status has no JSON declaration and doesn't
  declare the same body under a text type. Those declarations take precedence.
- A status that declares no body reads any response.

When a status declares a body, a missing or unmatched `Content-Type` returns `Other` .
An exact status takes precedence over a range such as `4XX` , which in turn takes
precedence over `default` . The declaration order doesn't affect that choice. For
example, an HTML `404` page returns `Other` when the `404` declaration requires a JSON
`Problem` ; the client doesn't try the `4XX` body as a fallback.

[Reading Responses](./client-generation.md#reading-responses) shows how the client
decodes these bodies, and
[Returning Responses](./server-generation.md#returning-responses) shows how the server
sends them.

A `text/event-stream` response delivers Server-Sent Events. The client reads it as
`oas3_gen_support::EventStream<oas3_gen_support::SseFrame<T>>` and the server returns
`futures::stream::BoxStream<'static, T>` , where `T` is the type of one event's data.
The petstore's `streamPetVitals` operation streams `VitalsReading` values this way. When
the schema describes the whole event, with properties drawn from `data` , `event` , `id`
, and `retry` , it's a frame type: the client reads it as an alias of `SseFrame` , the
server sends it as one complete Axum event, and the stream carries that type itself. In
a payload union, a stream member is named after its item type with a `Stream` suffix,
such as `ContentResponseOrAssistantEventFrameStream` . See
[Reading Server-Sent Events](./client-generation.md#reading-server-sent-events) for the
client side and
[Streaming Server-Sent Events](./server-generation.md#streaming-server-sent-events) for
the server side.

## Multipart Form Bodies

A `multipart/form-data` body uses a struct in `types.rs` . The client sends its
properties as separate parts, and the server reads those parts back into the struct.
Each part has the property's name and an encoding chosen from its schema and the
operation's `encoding` map.

Let's start with a body that uploads files next to a few text fields. The petstore's
`submitIntakeForm` operation sends the `IntakeForm` schema as `multipart/form-data` .
Here it is, trimmed to four of its properties and the requirements on them:

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

The generator produces a struct with `name: String` , `file: Option<Vec<u8>>` ,
`attachments: Vec<Vec<u8>>` , and `quality: Option<CoatQuality>` . The full schema also
has base64 text, numbers, dates, nested objects, and maps, and the operation's
`encoding` map changes how several of them are sent. The tables below describe those
encodings. See
[Sending Multipart Bodies](./client-generation.md#sending-multipart-bodies) and
[Receiving Multipart Bodies](./server-generation.md#receiving-multipart-bodies) for the
code that writes and reads them.

### Choosing the Part for Each Property

The following table shows how the generator encodes each kind of property when the media
type has no `encoding` entry for it:

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

An untyped property is generated as [`Vec<u8>`][rustdoc-vec] in a multipart body, and
`items: {}` produces `Vec<Vec<u8>>` . This lets an array carry several files under one
part name. The petstore's `uploadMedia` operation uses both forms: its untyped `cover`
property becomes `Option<Vec<u8>>` , and its `file` property, an array with `items: {}`
, becomes `Option<Vec<Vec<u8>>>` . This mapping applies only to schemas used exclusively
as multipart bodies. If the same schema is also a JSON response or another schema's
property, the field stays [`serde_json::Value`][rustdoc-serde-json-value] , so its JSON
form doesn't change, and the generator warns.

A value whose type depends on the data, such as an untagged union, goes as JSON unless
it's a string. That way a server can tell the string `"5"` from the number `5` .

### Encoding Objects

Each entry in a media type's `encoding` map is an OpenAPI *Encoding Object*. It changes
how one property is sent. Its `contentType` field sets the media type of that property's
parts:

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

The `style` , `explode` , and `allowReserved` fields switch a property to the
serialization used for query parameters. When any of them is present, `contentType` is
ignored for text values, and values aren't percent-encoded. File properties keep their
file parts. The following table applies to struct and map properties:

| Property | Encoding | Parts sent |
|---|---|---|
| Array | `explode: true` (the default for `form`) | One part per item |
| Array | `explode: false` | One part, with items joined by `,`, a space, or `\|` for `form`, `spaceDelimited`, or `pipeDelimited` |
| Object | `style: deepObject` | One part per member, named `property[member]` |
| Object | `style: form`, `explode: true` | One part per member, named by the member |
| Object | `style: form`, `explode: false` | One part of alternating names and values, such as `min,1,max,5` |

`IntakeForm` uses several of these rows. Its `ids` array is `pipeDelimited` with
`explode: false` , `filter` uses `deepObject` , `range` uses `form` with
`explode: false` , and `extras` uses `form` with `explode: true` .

This serialization has no way to escape its delimiters. An item that contains the
delimiter, such as `a,b` in a comma-joined array, arrives as two items. A `style` value
that `multipart/form-data` doesn't support, such as `matrix` , falls back to `form` with
a warning.

An Encoding Object can also declare `headers` for a part. The generated client doesn't
send part headers, and the generated server doesn't expose them. Part headers are
optional unless a header sets `required: true` , so the generator warns about each
required one.

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

The parser the generator uses doesn't keep a schema's `contentEncoding` , so a
`type: string` property with `contentEncoding: base64` is sent as a plain text part
rather than labelled `application/octet-stream` .

[reference-deprecated]: https://doc.rust-lang.org/reference/attributes/diagnostics.html#the-deprecated-attribute
[rustdoc-clone]: https://doc.rust-lang.org/std/clone/trait.Clone.html
[rustdoc-cookie]: https://docs.rs/cookie/0.18.2/cookie/
[rustdoc-debug]: https://doc.rust-lang.org/std/fmt/trait.Debug.html
[rustdoc-display]: https://doc.rust-lang.org/std/fmt/trait.Display.html
[rustdoc-expose-secret]: https://docs.rs/secrecy/0.10.3/secrecy/trait.ExposeSecret.html
[rustdoc-from-str]: https://doc.rust-lang.org/std/str/trait.FromStr.html
[rustdoc-http-status]: https://docs.rs/http/1.5.0/http/status/struct.StatusCode.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
[rustdoc-partial-eq]: https://doc.rust-lang.org/std/cmp/trait.PartialEq.html
[rustdoc-reqwest-response]: https://docs.rs/reqwest/0.13.5/reqwest/struct.Response.html
[rustdoc-secret-string]: https://docs.rs/secrecy/0.10.3/secrecy/type.SecretString.html
[rustdoc-serde-json-value]: https://docs.rs/serde_json/1.0.151/serde_json/enum.Value.html
[rustdoc-serde-serialize]: https://docs.rs/serde/1.0.229/serde/trait.Serialize.html
[rustdoc-string]: https://doc.rust-lang.org/std/string/struct.String.html
[rustdoc-unit]: https://doc.rust-lang.org/std/primitive.unit.html
[rustdoc-vec]: https://doc.rust-lang.org/std/vec/struct.Vec.html
[rustdoc-http-header-value]: https://docs.rs/http/1.5.0/http/header/struct.HeaderValue.html
