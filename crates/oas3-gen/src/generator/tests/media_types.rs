use super::support::{
  assert_contains, assert_contains_all, assert_not_contains, assert_occurs_at_least, generate_client, generate_server,
  generate_server_files, generate_types, make_orchestrator, make_orchestrator_with_api_name, make_server_orchestrator,
  parse_spec,
};
use crate::generator::GenerationTarget;

const MEDIA_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Media", "version": "1.0.0" },
  "paths": {
    "/photo": {
      "get": {
        "operationId": "getPhoto",
        "responses": {
          "200": { "description": "ok", "content": { "image/png": { "schema": { "type": "string", "format": "binary" } } } },
          "404": { "description": "missing", "content": { "application/problem+json": { "schema": { "$ref": "#/components/schemas/Problem" } } } }
        }
      },
      "put": {
        "operationId": "putPhoto",
        "requestBody": { "required": true, "content": { "image/png": { "schema": { "type": "string", "format": "binary" } } } },
        "responses": { "204": { "description": "stored" } }
      }
    },
    "/card": {
      "get": {
        "operationId": "getCard",
        "responses": { "200": { "description": "ok", "content": { "text/plain": { "schema": { "type": "string" } } } } }
      }
    },
    "/notes": {
      "put": {
        "operationId": "putNotes",
        "requestBody": { "required": true, "content": { "text/plain": { "schema": { "type": "string" } } } },
        "responses": { "204": { "description": "stored" } }
      },
      "post": {
        "operationId": "appendNotes",
        "requestBody": { "content": { "text/plain": { "schema": { "type": "string" } } } },
        "responses": { "204": { "description": "stored" } }
      }
    },
    "/readings": {
      "get": {
        "operationId": "streamReadings",
        "responses": { "200": { "description": "ok", "content": { "text/event-stream": { "schema": { "$ref": "#/components/schemas/Reading" } } } } }
      },
      "options": {
        "operationId": "describeReadings",
        "responses": { "204": { "description": "allowed" } }
      },
      "trace": {
        "operationId": "traceReadings",
        "responses": { "204": { "description": "echoed" } }
      }
    }
  },
  "components": {
    "schemas": {
      "Problem": { "type": "object", "properties": { "title": { "type": "string" } } },
      "Reading": { "type": "object", "required": ["id"], "properties": { "id": { "type": "string" } } }
    }
  }
}"##;

#[test]
fn server_sends_bodies_as_their_declared_media_type() {
  let server = generate_server(&make_server_orchestrator(parse_spec(MEDIA_SPEC)), "test.json");

  assert_contains_all(
    &server,
    &[
      (
        "axum::http::HeaderValue::from_static(",
        "non-JSON bodies set their Content-Type",
      ),
      ("\"image/png\"", "binary bodies carry their declared media type"),
      (
        "\"application/problem+json\"",
        "a +json body keeps its declared media type",
      ),
      ("axum::Json(body)", "+json bodies are still serialized as JSON"),
      ("\"text/plain\"", "text bodies carry their declared media type"),
      (
        "oas3_gen_support::ServerEvents<Reading>",
        "event streams return typed server events",
      ),
      (
        "axum::response::sse::Sse::new(",
        "event streams are sent with axum's Sse",
      ),
      (
        "axum::response::sse::Event::default().json_data(item)",
        "each streamed item is one JSON event",
      ),
    ],
  );
  assert_not_contains(
    &server,
    "oas3_gen_support::EventStream<",
    "the server never uses the client's event stream reader",
  );
}

#[test]
fn server_reads_text_and_binary_request_bodies() {
  let server = generate_server(&make_server_orchestrator(parse_spec(MEDIA_SPEC)), "test.json");

  assert_contains_all(
    &server,
    &[
      ("body: axum::body::Bytes", "binary bodies are extracted as bytes"),
      ("body: Vec::from(body)", "a required binary body becomes its bytes"),
      ("body: String", "text bodies are extracted as a string"),
      (
        "body: (!body.is_empty()).then_some(body)",
        "an optional text body is absent when empty",
      ),
    ],
  );
  assert_not_contains(
    &server,
    "Option<axum::body::Bytes>",
    "axum can't extract an optional body directly",
  );
  assert_not_contains(
    &server,
    "body: Option<String>",
    "axum can't extract an optional body directly",
  );
}

#[test]
fn server_routes_start_with_qualified_routing_functions() {
  let server = generate_server(&make_server_orchestrator(parse_spec(MEDIA_SPEC)), "test.json");

  assert_not_contains(&server, "routing::{", "no routing functions are imported");
  assert_contains_all(
    &server,
    &[
      ("axum::routing::get(", "a route can start with get"),
      ("axum::routing::put(", "a route can start with put"),
      (
        ".options(describe_readings::<S>)",
        "options is added to an existing route",
      ),
      (".trace(trace_readings::<S>)", "trace is added to an existing route"),
    ],
  );
}

#[test]
fn client_supports_every_http_method_and_streams_events() {
  let client = generate_client(
    &make_orchestrator_with_api_name(parse_spec(MEDIA_SPEC), "MediaClient", GenerationTarget::Client),
    "test.json",
  );

  assert_contains_all(
    &client,
    &[
      (
        "self.client.request(reqwest::Method::OPTIONS, url)",
        "OPTIONS goes through the generic request builder",
      ),
      (
        "self.client.request(reqwest::Method::TRACE, url)",
        "TRACE goes through the generic request builder",
      ),
      (
        "oas3_gen_support::EventStream<Reading>",
        "the client reads event streams",
      ),
    ],
  );
  assert_not_contains(
    &client,
    "oas3_gen_support::ServerEvents<",
    "the client never uses the server's event type",
  );
}

#[test]
fn nullable_array_of_union_stays_an_array() {
  let spec_json = r##"{
    "openapi": "3.1.0",
    "info": { "title": "Counters", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "CounterA": { "type": "object", "required": ["kind"], "properties": { "kind": { "type": "string", "const": "a" } } },
        "CounterB": { "type": "object", "required": ["kind"], "properties": { "kind": { "type": "string", "const": "b" } } },
        "Counters": {
          "description": "Counters, or null",
          "anyOf": [
            { "type": "array", "items": { "anyOf": [{ "$ref": "#/components/schemas/CounterA" }, { "$ref": "#/components/schemas/CounterB" }] } },
            { "type": "null" }
          ]
        },
        "Holder": { "type": "object", "properties": { "counters": { "$ref": "#/components/schemas/Counters" } } }
      }
    }
  }"##;

  let types = generate_types(&make_orchestrator(parse_spec(spec_json), true), "test.json").code;

  assert_contains_all(
    &types,
    &[
      ("pub type Counters = Vec<", "the nullable array keeps its array shape"),
      ("/// Counters, or null", "the alias keeps the outer description"),
      (
        "pub counters: Option<Counters>",
        "the field that uses it carries the nullability",
      ),
    ],
  );
  assert_not_contains(
    &types,
    "pub enum Counters ",
    "the item union is not promoted to the schema",
  );
}

const REQUEST_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Requests", "version": "1.0.0" },
  "security": [{ "Session": [] }],
  "paths": {
    "/pets/{petId}": {
      "parameters": [{ "name": "petId", "in": "path", "required": true, "schema": { "type": "string" } }],
      "patch": {
        "operationId": "patchPet",
        "requestBody": { "required": true, "content": { "application/merge-patch+json": { "schema": { "$ref": "#/components/schemas/Patch" } } } },
        "responses": {
          "200": { "description": "ok", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Patch" } } } },
          "404": { "description": "missing", "content": { "application/problem+json": { "schema": { "$ref": "#/components/schemas/Problem" } } } },
          "default": { "description": "error", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } }
        }
      }
    },
    "/notes": {
      "put": {
        "operationId": "putNotes",
        "requestBody": { "required": true, "content": { "text/plain": { "schema": { "type": "string", "maxLength": 20 } } } },
        "responses": { "204": { "description": "stored" } }
      }
    },
    "/signup": {
      "post": {
        "operationId": "signUp",
        "requestBody": { "required": true, "content": { "application/x-www-form-urlencoded": { "schema": { "$ref": "#/components/schemas/Patch" } } } },
        "responses": { "204": { "description": "stored" } }
      }
    },
    "/optout": {
      "post": {
        "operationId": "optOut",
        "requestBody": { "content": { "application/x-www-form-urlencoded": { "schema": { "$ref": "#/components/schemas/Patch" } } } },
        "responses": { "204": { "description": "stored" } }
      }
    },
    "/media": {
      "post": {
        "operationId": "uploadMedia",
        "requestBody": { "content": { "multipart/form-data": { "schema": { "type": "object", "properties": { "note": { "type": "string" } } } } } },
        "responses": { "204": { "description": "stored" } }
      }
    }
  },
  "components": {
    "securitySchemes": { "Session": { "type": "apiKey", "in": "cookie", "name": "session" } },
    "schemas": {
      "Patch": { "type": "object", "properties": { "name": { "type": "string" } } },
      "Problem": { "type": "object", "properties": { "title": { "type": "string" } } },
      "Error": { "type": "object", "properties": { "message": { "type": "string" } } }
    }
  }
}"##;

#[test]
fn client_labels_bodies_with_their_declared_media_type() {
  let client = generate_client(
    &make_orchestrator_with_api_name(parse_spec(REQUEST_SPEC), "RequestsClient", GenerationTarget::Client),
    "test.json",
  );
  let types = generate_types(&make_orchestrator(parse_spec(REQUEST_SPEC), false), "test.json").code;

  assert_contains_all(
    &client,
    &[
      (
        "\"application/merge-patch+json\"",
        "a +json body is labelled with its declared media type",
      ),
      ("\"text/plain\"", "a text body is labelled with its declared media type"),
      (".form(&request.body)", "a form body keeps reqwest's urlencoded label"),
    ],
  );
  assert_contains(
    &types,
    "#[validate(length(max = 20u64))]",
    "a primitive body's own constraints are validated",
  );
}

#[test]
fn server_rejects_malformed_bodies_with_bad_request() {
  let server = generate_server(&make_server_orchestrator(parse_spec(REQUEST_SPEC)), "test.json");

  assert_contains_all(
    &server,
    &[
      (
        "axum::extract::rejection::JsonRejection",
        "JSON bodies are extracted with their rejection",
      ),
      (
        "axum::extract::rejection::FormRejection",
        "form bodies are extracted with their rejection",
      ),
      ("Ok(axum::Json(body)) => body", "an accepted JSON body is unwrapped"),
      (
        "headers.contains_key(axum::http::header::CONTENT_TYPE)",
        "an optional multipart body rejects other media types",
      ),
      (
        "Err(_) if !headers.contains_key(axum::http::header::CONTENT_TYPE) => None",
        "an optional form body is absent when the request has no media type",
      ),
      (
        "axum_extra::extract::cookie::Cookie::split_parse",
        "cookie keys are read without percent-decoding",
      ),
      (
        "\"application/problem+json\"",
        "a union body keeps the media type its status declares",
      ),
    ],
  );
  assert_not_contains(
    &server,
    "CookieJar::from_headers",
    "the jar would percent-decode cookie keys",
  );
}

#[test]
fn exploded_query_arrays_travel_as_repeated_pairs() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Search", "version": "1.0.0" },
    "paths": {
      "/pets": {
        "get": {
          "operationId": "searchPets",
          "parameters": [
            { "name": "tags", "in": "query", "schema": { "type": "array", "items": { "type": "string" } } },
            { "name": "years", "in": "query", "schema": { "type": "array", "items": { "type": "integer" } } }
          ],
          "responses": { "204": { "description": "ok" } }
        }
      }
    }
  }"#;

  let client = generate_client(
    &make_orchestrator_with_api_name(parse_spec(spec_json), "SearchClient", GenerationTarget::Client),
    "test.json",
  );
  let server = generate_server(&make_server_orchestrator(parse_spec(spec_json)), "test.json");

  assert_contains_all(
    &client,
    &[
      (
        "serde_html_form::Serializer::new(&mut url.query_pairs_mut())",
        "the client writes one pair per array item onto the URL",
      ),
      (
        "if url.query() == Some(\"\")",
        "a query with nothing to send leaves no empty `?`",
      ),
    ],
  );
  assert_not_contains(&client, ".query(&", "reqwest's serde_urlencoded can't repeat a name");
  assert_contains(
    &server,
    "axum_extra::extract::Query<SearchPetsRequestQuery>",
    "the server collects repeated pairs into arrays",
  );
  assert_not_contains(
    &server,
    "extract::{Path, Query, State}",
    "axum's `Query` can't read repeated pairs",
  );
}

const PARAMETER_SPEC: &str = r##"{
  "openapi": "3.1.0",
  "info": { "title": "Parameters", "version": "1.0.0" },
  "security": [{ "Key": [] }],
  "paths": {
    "/pets": {
      "get": {
        "operationId": "searchPets",
        "security": [{ "Session": [] }],
        "parameters": [
          { "name": "q", "in": "query", "required": true, "allowReserved": true, "schema": { "type": "string" } },
          { "name": "page", "in": "query", "schema": { "type": "integer" } },
          { "name": "theme", "in": "cookie", "schema": { "type": "string", "enum": ["light", "dark"] } },
          { "name": "seen", "in": "cookie", "required": true, "schema": { "type": "array", "items": { "type": "integer" } } }
        ],
        "callbacks": {
          "onMatch": {
            "{$request.query.q}": { "post": { "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Tagged" } } } }, "responses": { "204": { "description": "ok" } } } }
          }
        },
        "responses": { "200": { "description": "ok", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Tagged" } } } } }
      }
    },
    "/plain": {
      "get": {
        "operationId": "plain",
        "parameters": [{ "name": "page", "in": "query", "schema": { "type": "integer" } }],
        "responses": { "204": { "description": "ok" } }
      }
    }
  },
  "components": {
    "securitySchemes": {
      "Key": { "type": "apiKey", "in": "header", "name": "X-Key" },
      "Session": { "type": "apiKey", "in": "cookie", "name": "session" }
    },
    "schemas": {
      "Tagged": {
        "type": "object",
        "required": ["tags"],
        "properties": {
          "tags": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
          "labels": { "type": "array", "items": { "type": "string" }, "uniqueItems": true }
        }
      }
    }
  }
}"##;

#[test]
fn cookie_parameters_render_into_and_parse_from_the_cookie_header() {
  let spec = parse_spec(PARAMETER_SPEC);
  let client = generate_client(
    &make_orchestrator_with_api_name(spec.clone(), "ParametersClient", GenerationTarget::Client),
    "test.json",
  );
  let client_types = generate_types(&make_orchestrator(spec.clone(), false), "test.json").code;
  let (server, server_types) = generate_server_files(&make_server_orchestrator(spec), "test.json");

  assert_contains_all(
    &client,
    &[
      (
        "reqwest::cookie::CookieStore::cookies(&*self.cookies, &url)",
        "stored cookies merge into a request's own `Cookie` header",
      ),
      (
        ".chain(request.cookie.cookie_pairs())",
        "cookie parameters follow the stored cookies",
      ),
      (
        "req_builder.header(reqwest::header::COOKIE, cookie)",
        "the merged header is sent",
      ),
    ],
  );
  assert_contains(
    &client_types,
    "pub fn cookie_pairs(&self) -> Vec<String>",
    "the client renders its cookies",
  );
  assert_contains_all(
    &client_types,
    &[
      (
        "reqwest_cookie_store::RawCookie::new(",
        "each pair is a cookie from the `cookie` crate",
      ),
      (".encoded()", "cookie values are percent-encoded"),
    ],
  );
  assert_contains_all(
    &server_types,
    &[
      (
        "for SearchPetsRequestCookie",
        "the server parses its cookie struct from the request headers",
      ),
      (
        "missing required cookie `seen`",
        "a missing required cookie is an error",
      ),
      (
        "invalid value for cookie `theme`",
        "a value that doesn't parse is an error",
      ),
      (
        "axum_extra::extract::cookie::Cookie::split_parse_encoded",
        "cookie values are percent-decoded",
      ),
    ],
  );
  assert_contains(
    &server,
    "SearchPetsRequestCookie::try_from(&headers)",
    "the handler reads cookie parameters before the service runs",
  );
}

#[test]
fn allow_reserved_queries_are_encoded_like_any_other() {
  let client = generate_client(
    &make_orchestrator_with_api_name(parse_spec(PARAMETER_SPEC), "ParametersClient", GenerationTarget::Client),
    "test.json",
  );

  assert_occurs_at_least(
    &client,
    "serde_html_form::Serializer::new(&mut url.query_pairs_mut())",
    2,
    "an allowReserved parameter is sent like any other",
  );
  assert_not_contains(&client, "set_query(Some(", "no operation writes its own query string");
}

#[test]
fn unique_items_reject_repeated_items() {
  let types = generate_types(&make_orchestrator(parse_spec(PARAMETER_SPEC), true), "test.json").code;

  assert_contains_all(
    &types,
    &[
      (
        "#[serde_as(as = \"serde_with::SetPreventDuplicates<_>\")]",
        "a required set rejects a repeated item",
      ),
      (
        "#[serde_as(as = \"Option<serde_with::SetPreventDuplicates<_>>\")]",
        "an optional set rejects a repeated item",
      ),
      ("#[serde_with::serde_as]", "the struct opts into serde_as"),
    ],
  );
}

#[test]
fn callbacks_become_server_receivers_without_the_api_security() {
  let spec = parse_spec(PARAMETER_SPEC);
  let server = generate_server(&make_server_orchestrator(spec.clone()), "test.json");
  let client = generate_client(
    &make_orchestrator_with_api_name(spec, "ParametersClient", GenerationTarget::Client),
    "test.json",
  );

  assert_contains_all(
    &server,
    &[
      (
        "\"/callbacks/searchPets/onMatch\"",
        "the receiver is routed under its operation",
      ),
      ("fn search_pets_on_match(", "the receiver is a trait method"),
    ],
  );
  let receiver = &server[server.find("async fn search_pets_on_match").expect("receiver handler")..];
  let receiver = &receiver[..receiver.find("\n}").expect("handler end")];
  assert_not_contains(
    receiver,
    "credentials",
    "a callback doesn't inherit the API's top-level security",
  );
  assert_not_contains(&client, "search_pets_on_match", "the client never receives callbacks");
}
