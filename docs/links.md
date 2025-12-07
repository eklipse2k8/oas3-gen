# Link Object Support

Support for [OpenAPI Link Objects](https://spec.openapis.org/oas/v3.1.1.html#link-object) enables type-safe construction of follow-up requests by merging response data with original request data.

## Overview

When an OpenAPI response defines links, the generator produces:

1. **Links struct** - Contains `Option<TargetRequest>` fields for each link
2. **Tuple response variants** - `Ok(Body, Links)` instead of `Ok(Body)`
3. **Accessor methods** - Ergonomic access to response body and linked requests
4. **TryFrom implementations** - Convert responses directly to body or linked requests

## Generated Code Patterns

### Response Enum with Links

```rust
pub enum CreateBurgerResponse {
    Ok(Burger, CreateBurgerResponseOkLinks),
    UnprocessableEntity(Error),
    InternalServerError(Error),
    Unknown,
}
```

### Links Struct

```rust
pub struct CreateBurgerResponseOkLinks {
    pub locate_burger: Option<LocateBurgerRequest>,
    pub another_locate_burger: Option<LocateBurgerRequest>,
}
```

### Accessor Methods

```rust
impl CreateBurgerResponse {
    pub fn body(&self) -> Option<&Burger> {
        match self {
            Self::Ok(body, _) => Some(body),
            _ => None,
        }
    }

    pub fn into_body(self) -> Option<Burger> {
        match self {
            Self::Ok(body, _) => Some(body),
            _ => None,
        }
    }

    pub fn to_locate_burger_request(&self) -> Option<LocateBurgerRequest> {
        match self {
            Self::Ok(_, links) => links.locate_burger.clone(),
            _ => None,
        }
    }
}
```

### TryFrom Implementations

```rust
impl TryFrom<CreateBurgerResponse> for Burger {
    type Error = CreateBurgerResponse;

    fn try_from(response: CreateBurgerResponse) -> Result<Self, Self::Error> {
        match response {
            CreateBurgerResponse::Ok(body, _) => Ok(body),
            other => Err(other),
        }
    }
}

impl TryFrom<CreateBurgerResponse> for LocateBurgerRequest {
    type Error = CreateBurgerResponse;

    fn try_from(response: CreateBurgerResponse) -> Result<Self, Self::Error> {
        match response {
            CreateBurgerResponse::Ok(_, links) => {
                links.locate_burger.ok_or(CreateBurgerResponse::Unknown)
            }
            other => Err(other),
        }
    }
}
```

## Usage Examples

### Pattern Matching

```rust
let response = client.create_burger(request).await?;

if let CreateBurgerResponse::Ok(burger, links) = response {
    println!("Created burger: {:?}", burger);
    if let Some(locate_req) = links.locate_burger {
        let located = client.locate_burger(locate_req).await?;
    }
}
```

### Via TryFrom (Most Ergonomic)

```rust
let response = client.create_burger(request).await?;
let burger: Burger = response.try_into()?;

let response = client.create_burger(request).await?;
let locate_req: LocateBurgerRequest = response.try_into()?;
let located = client.locate_burger(locate_req).await?;
```

### Via Accessor Methods

```rust
let response = client.create_burger(request).await?;

if let Some(burger) = response.body() {
    println!("Created: {:?}", burger);
}

if let Some(locate_req) = response.to_locate_burger_request() {
    let located = client.locate_burger(locate_req).await?;
}
```

## Supported Runtime Expressions

Link parameters can use runtime expressions to extract values from the response body or original request:

| Expression | Description | Example |
|------------|-------------|---------|
| `$response.body#/path` | Value from response body JSON path | `$response.body#/id` |
| `$request.query.name` | Original request query parameter | `$request.query.filter` |
| `$request.path.name` | Original request path parameter | `$request.path.userId` |
| `$request.header.name` | Original request header | `$request.header.X-Request-Id` |
| `$request.body` | Original request body | `$request.body` |
| `$request.body#/path` | Value from request body JSON path | `$request.body#/nested/field` |
| Literal values | Static values (no `$` prefix) | `"static-value"` |

### JSON Pointer Syntax

The `#/path` portion follows [RFC 6901 JSON Pointer](https://datatracker.ietf.org/doc/html/rfc6901) syntax:

- `/foo/bar` - Access nested fields
- `/foo/0/bar` - Access array element then field
- `~0` - Escaped `~` character
- `~1` - Escaped `/` character

## Breaking Change

**`parse_response` is now an instance method:**

Before (v0.21 and earlier):
```rust
let parsed = CreateBurgerRequest::parse_response(response).await?;
```

After (v0.22+):
```rust
let parsed = request.parse_response(response).await?;
```

This change enables the response parser to access original request data when constructing linked requests.

## Limitations

The following are not currently supported:

- `operationRef` (cross-document references) - Only `operationId` is supported
- `$response.header.*` expressions - Would require storing headers in response
- `$url`, `$method`, `$statusCode` runtime expressions
- Cookie parameter expressions
- Server variable substitution
- `requestBody` field in links

### Missing Source Fields

When a runtime expression references a field that doesn't exist in the response body or original request:

- The link field becomes `None`
- No warning is generated
- The rest of the response parsing succeeds

This graceful degradation allows partial link construction when some fields are unavailable.

## OpenAPI Spec Example

```json
{
  "paths": {
    "/burgers": {
      "post": {
        "operationId": "createBurger",
        "responses": {
          "200": {
            "content": {
              "application/json": {
                "schema": { "$ref": "#/components/schemas/Burger" }
              }
            },
            "links": {
              "LocateBurger": {
                "operationId": "locateBurger",
                "parameters": {
                  "burgerId": "$response.body#/id"
                },
                "description": "Go and get a tasty burger"
              }
            }
          }
        }
      }
    },
    "/burgers/{burgerId}": {
      "get": {
        "operationId": "locateBurger",
        "parameters": [
          {
            "name": "burgerId",
            "in": "path",
            "required": true,
            "schema": { "type": "string" }
          }
        ]
      }
    }
  }
}
```

## Implementation Details

### Key Files

- [ast/links.rs](../crates/oas3-gen/src/generator/ast/links.rs) - Link AST types
- [converter/links.rs](../crates/oas3-gen/src/generator/converter/links.rs) - Link extraction
- [converter/runtime_expression.rs](../crates/oas3-gen/src/generator/converter/runtime_expression.rs) - Expression parser
- [codegen/links.rs](../crates/oas3-gen/src/generator/codegen/links.rs) - Code generation

### Flow

1. **Extract** - `LinkConverter` extracts `LinkDef` from OpenAPI response
2. **Parse** - Runtime expressions parsed into `RuntimeExpression` enum
3. **Resolve** - Link targets resolved to `StructToken` request types
4. **Generate** - Links struct, accessor methods, and TryFrom impls generated
5. **Construct** - At runtime, `parse_response` builds links from response + request data
