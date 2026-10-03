use std::collections::HashMap;

use super::support::{
  assert_contains, assert_contains_all, assert_not_contains, assert_occurs_at_least, generate_client, generate_server,
  generate_types, make_orchestrator, make_orchestrator_with_api_name, make_orchestrator_with_customizations,
  make_orchestrator_with_fn_name_overrides, make_orchestrator_with_ops, make_server_orchestrator, parse_spec,
  string_set,
};
use crate::generator::{GenerationTarget, ServerModMode, ast::ApiMetadata};

type PresenceCheck<'a> = (&'a str, usize, &'a str);
type AbsenceCheck<'a> = (&'a str, &'a str);
type EnumDedupCase<'a> = (&'a str, Vec<PresenceCheck<'a>>, Vec<AbsenceCheck<'a>>);

#[test]
fn test_metadata_and_header_generation() {
  let spec = parse_spec(include_str!("../../../fixtures/basic_api.json"));
  let metadata = ApiMetadata::from(&spec);

  assert_eq!(metadata.title, "Basic Test API", "title mismatch");
  assert_eq!(metadata.version, "1.0.0", "version mismatch");
  assert_eq!(
    metadata.description.as_deref(),
    Some("A test API.\nWith multiple lines.\nFor testing documentation."),
    "description mismatch"
  );

  let orchestrator = make_orchestrator(spec, false);
  let output = generate_types(&orchestrator, "/path/to/spec.json");
  assert_contains_all(
    &output.code,
    &[
      ("AUTO-GENERATED CODE - DO NOT EDIT!", "auto-generated marker"),
      ("//! Basic Test API", "title in header"),
      ("//! Source: /path/to/spec.json", "source path"),
      ("//! Version: 1.0.0", "version in header"),
      ("//! A test API.", "description in header"),
      ("#![allow(clippy::doc_markdown)]", "clippy allow"),
      (
        "A test API.\n//! With multiple lines.\n//! For testing documentation.",
        "multiline description formatting",
      ),
    ],
  );
}

#[test]
fn test_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let excluded = string_set(&["admin_action"]);

  let full_orchestrator = make_orchestrator(parse_spec(spec_json), false);
  let full = generate_types(&full_orchestrator, "test.json");

  let filtered_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), false, None, Some(&excluded));
  let filtered = generate_types(&filtered_orchestrator, "test.json");

  assert_eq!(full.operations_converted, 3, "full spec should have 3 ops");
  assert_eq!(
    filtered.operations_converted, 2,
    "excluded admin_action should leave 2 ops"
  );
  assert_not_contains(
    &filtered.code,
    "admin_action",
    "admin_action should be excluded from generated code",
  );
  assert_contains(
    &full.code,
    "AdminActionRequest",
    "full code should contain AdminActionRequest",
  );
  assert_not_contains(
    &filtered.code,
    "AdminActionRequest",
    "filtered code should not contain AdminActionRequest",
  );
  assert_contains(
    &filtered.code,
    "UserList",
    "filtered code should still contain UserList",
  );
  assert_contains(&filtered.code, "User", "filtered code should still contain User");
}

#[test]
fn test_all_schemas_overrides_operation_filtering() {
  let spec_json = include_str!("../../../fixtures/operation_filtering.json");
  let only = string_set(&["list_users"]);

  let without_all_schemas_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), false, Some(&only), None);
  let without_all_schemas = generate_types(&without_all_schemas_orchestrator, "test.json");

  let with_all_schemas_orchestrator = make_orchestrator_with_ops(parse_spec(spec_json), true, Some(&only), None);
  let with_all_schemas = generate_types(&with_all_schemas_orchestrator, "test.json");

  assert_eq!(without_all_schemas.operations_converted, 1, "without all_schemas: 1 op");
  assert_eq!(with_all_schemas.operations_converted, 1, "with all_schemas: still 1 op");

  assert_contains(
    &without_all_schemas.code,
    "UserList",
    "without all_schemas should contain UserList",
  );
  assert_contains(
    &without_all_schemas.code,
    "User",
    "without all_schemas should contain User",
  );
  assert_not_contains(
    &without_all_schemas.code,
    "AdminResponse",
    "without all_schemas should not contain AdminResponse",
  );
  assert_not_contains(
    &without_all_schemas.code,
    "UnreferencedSchema",
    "without all_schemas should not contain UnreferencedSchema",
  );

  assert_contains_all(
    &with_all_schemas.code,
    &[
      ("UserList", "with all_schemas should contain UserList"),
      ("User", "with all_schemas should contain User"),
      ("AdminResponse", "with all_schemas should contain AdminResponse"),
      (
        "UnreferencedSchema",
        "with all_schemas should contain UnreferencedSchema",
      ),
    ],
  );

  assert_eq!(
    without_all_schemas.orphaned_schemas_count, 2,
    "without all_schemas: 2 orphaned"
  );
  assert_eq!(
    with_all_schemas.orphaned_schemas_count, 0,
    "with all_schemas: 0 orphaned"
  );
}

#[test]
fn test_fn_name_override_renames_derived_types() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {
      "/users": {
        "get": {
          "operationId": "listUsers",
          "parameters": [
            { "name": "limit", "in": "query", "schema": { "type": "integer" } }
          ],
          "responses": {
            "200": {
              "description": "Success",
              "content": { "application/json": { "schema": { "type": "string" } } }
            }
          }
        }
      }
    }
  }"#;

  let overrides = HashMap::from([("listUsers".to_string(), "fetch_all_users".to_string())]);
  let orchestrator = make_orchestrator_with_fn_name_overrides(parse_spec(spec_json), false, overrides);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains_all(
    &output.code,
    &[
      ("FetchAllUsersRequest", "request type should derive from the override"),
      (
        "ApiResponse<String>",
        "the shared response enum should be instantiated with the operation's body type",
      ),
    ],
  );
  assert_not_contains(
    &output.code,
    "ListUsersRequest",
    "request type should not use the original operation ID",
  );
  assert_not_contains(
    &output.code,
    "ListUsersResponse",
    "response type should not use the original operation ID",
  );
}

#[test]
fn test_api_name_overrides_client_struct_name() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Swagger Petstore", "version": "1.0.0" },
    "paths": {
      "/pets": {
        "get": {
          "operationId": "listPets",
          "responses": {
            "200": {
              "description": "Success",
              "content": { "application/json": { "schema": { "type": "string" } } }
            }
          }
        }
      }
    }
  }"#;

  let default = generate_client(&make_orchestrator(parse_spec(spec_json), false), "test.json");
  assert_contains(
    &default,
    "pub struct SwaggerPetstoreClient",
    "client name should derive from the title",
  );

  let orchestrator = make_orchestrator_with_api_name(parse_spec(spec_json), "PetStoreClient", GenerationTarget::Client);
  let client = generate_client(&orchestrator, "test.json");
  assert_contains_all(
    &client,
    &[
      ("pub struct PetStoreClient", "struct should use the overridden name"),
      ("impl PetStoreClient", "impl block should use the overridden name"),
    ],
  );
  assert_not_contains(
    &client,
    "SwaggerPetstoreClient",
    "title-derived name should not appear when overridden",
  );
}

#[test]
fn test_api_name_overrides_server_trait_name() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Swagger Petstore", "version": "1.0.0" },
    "paths": {
      "/pets": {
        "get": {
          "operationId": "listPets",
          "responses": {
            "200": {
              "description": "Success",
              "content": { "application/json": { "schema": { "type": "string" } } }
            }
          }
        }
      }
    }
  }"#;

  let orchestrator = make_orchestrator_with_api_name(parse_spec(spec_json), "PetStoreApi", GenerationTarget::Server);
  let server = generate_server(&orchestrator, "test.json");
  assert_contains_all(
    &server,
    &[
      (
        "pub trait PetStoreApi: Send + Sync",
        "trait should use the overridden name",
      ),
      (
        "S: PetStoreApi + Clone + Send + Sync + 'static",
        "handler and router bounds should use the overridden name",
      ),
    ],
  );
  assert_not_contains(
    &server,
    "ApiServer",
    "default trait name should not appear when overridden",
  );
}

#[test]
fn unsupported_security_schemes_warn_once_each() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Mixed Auth", "version": "1.0.0" },
    "security": [{ "BasicAuth": [] }],
    "paths": {
      "/a": { "get": { "operationId": "a", "responses": { "204": { "description": "ok" } } } },
      "/b": { "get": { "operationId": "b", "responses": { "204": { "description": "ok" } } } },
      "/c": {
        "get": {
          "operationId": "c",
          "security": [{ "Missing": [] }, { "ApiKeyAuth": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      }
    },
    "components": {
      "securitySchemes": {
        "BasicAuth": { "type": "http", "scheme": "basic" },
        "ApiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" }
      }
    }
  }"#;

  let output = make_server_orchestrator(parse_spec(spec_json))
    .generate(&ServerModMode::default(), "test.json")
    .expect("server generation should succeed");
  let warnings = output
    .stats
    .warnings
    .iter()
    .map(ToString::to_string)
    .collect::<Vec<_>>();
  assert_eq!(
    warnings,
    [
      "Security scheme 'BasicAuth' gets no credentials: only `apiKey` schemes and `http` schemes using `bearer` are generated, and it has type `http` with scheme `basic`",
      "Security scheme 'Missing' gets no credentials: it is not declared in `components.securitySchemes`",
    ],
    "each unsupported scheme should warn once"
  );
}

#[test]
fn credentials_struct_names_avoid_schema_names() {
  let spec_json = r##"{
    "openapi": "3.1.0",
    "info": { "title": "Collisions", "version": "1.0.0" },
    "security": [{ "ApiKeyAuth": [] }],
    "paths": {
      "/creds": {
        "get": {
          "operationId": "getCreds",
          "responses": {
            "200": {
              "description": "ok",
              "content": {
                "application/json": { "schema": { "$ref": "#/components/schemas/ApiKeyAuthCredentials" } }
              }
            }
          }
        }
      }
    },
    "components": {
      "securitySchemes": { "ApiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" } },
      "schemas": {
        "ApiKeyAuthCredentials": { "type": "object", "properties": { "label": { "type": "string" } } }
      }
    }
  }"##;

  let types = generate_types(&make_server_orchestrator(parse_spec(spec_json)), "test.json").code;
  assert_contains_all(
    &types,
    &[
      ("pub struct ApiKeyAuthCredentials {", "the schema keeps its name"),
      (
        "pub struct ApiKeyAuthCredentials2 {",
        "the credentials struct takes a suffix",
      ),
      (
        "pub credentials: ApiKeyAuthCredentials2,",
        "the request holds the credentials struct",
      ),
    ],
  );
}

#[test]
fn api_key_edge_cases_keep_credentials_usable() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Edge Cases", "version": "1.0.0" },
    "paths": {
      "/mixed": {
        "get": {
          "operationId": "mixed",
          "security": [{ "ApiKeyAuth": [], "BasicAuth": [] }],
          "parameters": [{ "name": "credentials", "in": "query", "schema": { "type": "string" } }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/cookies": {
        "get": {
          "operationId": "cookies",
          "security": [{ "CookieA": [], "CookieB": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/forward": {
        "get": {
          "operationId": "forward",
          "security": [{ "Client": [] }, { "CookieA": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/reverse": {
        "get": {
          "operationId": "reverse",
          "security": [{ "CookieA": [] }, { "Client": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      }
    },
    "components": {
      "securitySchemes": {
        "ApiKeyAuth": { "type": "apiKey", "in": "header", "name": "X-Api-Key" },
        "BasicAuth": { "type": "http", "scheme": "basic" },
        "Client": { "type": "apiKey", "in": "header", "name": "X-Client" },
        "CookieA": { "type": "apiKey", "in": "cookie", "name": "a" },
        "CookieB": { "type": "apiKey", "in": "cookie", "name": "b" }
      }
    }
  }"#;

  let client = generate_client(&make_orchestrator(parse_spec(spec_json), false), "test.json");
  assert_contains_all(
    &client,
    &[
      (
        "pub api_key_auth: Option<secrecy::SecretString>,",
        "an API key paired with an unsupported scheme stays on the client",
      ),
      (
        "pub client_2: Option<secrecy::SecretString>,",
        "a scheme named after a client member takes a suffix",
      ),
      (
        "pub cookies: std::sync::Arc<reqwest_cookie_store::CookieStoreMutex>,",
        "cookie keys live in a reqwest cookie store",
      ),
      (
        ".cookie_provider(std::sync::Arc::clone(&cookies))",
        "the reqwest client reads the cookie store",
      ),
      ("RawCookie::new(\"a\", api_key.into())", "the first cookie key"),
      ("RawCookie::new(\"b\", api_key.into())", "the second cookie key"),
      (".finish_non_exhaustive()", "Debug skips the cookie store"),
    ],
  );
  assert_not_contains(
    &client,
    "reqwest::header::COOKIE",
    "requests leave the Cookie header to reqwest",
  );

  let server = make_server_orchestrator(parse_spec(spec_json));
  let server_types = generate_types(&server, "test.json").code;
  assert_contains_all(
    &server_types,
    &[
      (
        "pub api_key_auth: Option<secrecy::SecretString>,",
        "an API key paired with an unsupported scheme is optional",
      ),
      (
        "pub credentials_2: ApiKeyAuthCredentials,",
        "the credentials field avoids a parameter named credentials",
      ),
      (
        "pub struct ClientAndCookieACredentials {",
        "reordered alternatives share one struct",
      ),
      (
        "pub client: Option<secrecy::SecretString>,",
        "server fields don't avoid client members",
      ),
    ],
  );
  for absent in [
    "ClientAndCookieACredentials2",
    "CookieAAndClientCredentials",
    "client_2",
  ] {
    assert_not_contains(&server_types, absent, "canonical server credentials");
  }

  let server_code = generate_server(&server, "test.json");
  assert_contains(
    &server_code,
    "credentials_2: ApiKeyAuthCredentials,",
    "the handler extracts the renamed credentials field",
  );
  assert_not_contains(&server_code, "fn reject", "handlers use axum's rejections");
}

#[test]
fn authorization_and_standard_headers_follow_http() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Tokens", "version": "1.0.0" },
    "paths": {
      "/bearer": {
        "get": {
          "operationId": "bearer",
          "security": [{ "BearerAuth": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/token": {
        "get": {
          "operationId": "token",
          "security": [{ "TokenAuth": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/either": {
        "get": {
          "operationId": "either",
          "security": [{ "BearerAuth": [] }, { "TokenAuth": [] }],
          "responses": { "204": { "description": "ok" } }
        }
      },
      "/forwarded": {
        "get": {
          "operationId": "forwarded",
          "parameters": [
            { "name": "Authorization", "in": "header", "required": true, "schema": { "type": "string" } },
            { "name": "accept", "in": "header", "schema": { "type": "string" } },
            { "name": "Content-Type", "in": "header", "schema": { "type": "string" } },
            { "name": "If-Match", "in": "header", "schema": { "type": "string" } },
            { "name": "X-Trace", "in": "header", "schema": { "type": "string" } }
          ],
          "responses": {
            "204": { "description": "ok", "headers": { "ETag": { "schema": { "type": "string" } } } }
          }
        }
      }
    },
    "components": {
      "securitySchemes": {
        "BearerAuth": { "type": "http", "scheme": "Bearer", "bearerFormat": "JWT" },
        "TokenAuth": { "type": "apiKey", "in": "header", "name": "Authorization" }
      }
    }
  }"#;

  let client = make_orchestrator(parse_spec(spec_json), false);
  let client_types = generate_types(&client, "test.json").code;
  assert_contains_all(
    &client_types,
    &[
      (
        "pub const X_TRACE: http::HeaderName",
        "a header http doesn't name keeps its generated constant",
      ),
      (
        "map.insert(http::header::IF_MATCH, header_value);",
        "a standard request header uses the http constant",
      ),
      (
        ".get(http::header::ETAG)",
        "a standard response header uses the http constant",
      ),
    ],
  );
  for absent in [
    "const AUTHORIZATION",
    "const IF_MATCH",
    "const ETAG",
    "pub authorization:",
    "pub accept:",
    "pub content_type:",
  ] {
    assert_not_contains(
      &client_types,
      absent,
      "standard headers get no constants, and ignored header parameters get no fields",
    );
  }

  let client_code = generate_client(&client, "test.json");
  assert_contains_all(
    &client_code,
    &[
      (
        "pub fn with_bearer_auth(mut self, token: impl Into<String>) -> Self {",
        "the bearer token setter",
      ),
      (
        ".bearer_auth(secrecy::ExposeSecret::expose_secret(token));",
        "reqwest attaches the bearer token",
      ),
      (
        "http::header::AUTHORIZATION,",
        "an API key in the Authorization header uses the http constant",
      ),
      (
        "} else if let Some(token) = &self.bearer_auth {",
        "a request carries one Authorization credential, and the last declared wins",
      ),
    ],
  );

  let server = make_server_orchestrator(parse_spec(spec_json));
  let server_types = generate_types(&server, "test.json").code;
  assert_contains(
    &server_types,
    "`BearerAuth` bearer token, read from the `Authorization` header.",
    "credential docs name the bearer token",
  );
  assert_not_contains(
    &server_types,
    "const AUTHORIZATION",
    "types.rs leaves Authorization to http",
  );

  let server_code = generate_server(&server, "test.json");
  assert_contains_all(
    &server_code,
    &[
      (
        ".filter(|(scheme, _)| scheme.eq_ignore_ascii_case(\"bearer\"))",
        "the Bearer scheme matches in any case",
      ),
      (
        "\"missing bearer token in the `Authorization` header\"",
        "a missing bearer token is rejected",
      ),
      (
        "\"missing API key in the `Authorization` header\"",
        "a missing Authorization API key is rejected",
      ),
    ],
  );
  assert_occurs_at_least(
    &server_code,
    ".get(http::header::AUTHORIZATION)",
    2,
    "both extractors read the http constant",
  );
}

#[test]
fn test_content_types_generation() {
  let orchestrator = make_orchestrator(parse_spec(include_str!("../../../fixtures/content_types.json")), false);
  let output = generate_types(&orchestrator, "test.json");
  assert_contains_all(
    &output.code,
    &[
      (
        "json_with_diagnostics",
        "JSON handling for application/json should be generated",
      ),
      ("req.text().await?", "text handling for text/plain should be generated"),
      (
        "req.bytes().await?",
        "binary handling for image/png should be generated",
      ),
    ],
  );
}

#[test]
fn test_enum_deduplication() {
  let cases: [EnumDedupCase<'_>; 2] = [
    (
      include_str!("../../../fixtures/enum_deduplication.json"),
      vec![
        ("pub enum Status", 1, "Status enum should be defined exactly once"),
        ("pub status: Option<Status>", 1, "StructA should use Status"),
        (
          "status: Option<Status>",
          3,
          "StructA, StructB, and reordered StructC should all use Status",
        ),
      ],
      vec![(
        "pub enum StructCStatus",
        "Reordered enum values should still dedupe to the shared Status enum",
      )],
    ),
    (
      include_str!("../../../fixtures/relaxed_enum_deduplication.json"),
      vec![
        ("pub enum Status", 1, "Status enum should be defined"),
        ("pub enum ComplexStatusStatus", 1, "Outer enum should be defined"),
        ("Known(Status)", 1, "Outer enum should wrap Status"),
      ],
      vec![("pub enum ComplexStatusStatusKnown", "Inner enum should be deduplicated")],
    ),
  ];

  for (spec_json, presence_checks, absence_checks) in cases {
    let orchestrator = make_orchestrator(parse_spec(spec_json), true);
    let output = generate_types(&orchestrator, "test.json");
    for (pattern, expected_count, context) in presence_checks {
      assert_occurs_at_least(&output.code, pattern, expected_count, context);
    }
    for (pattern, context) in absence_checks {
      assert_not_contains(&output.code, pattern, context);
    }
  }
}

#[test]
fn preserves_schema_property_enum_and_collection_order() {
  let spec_json = r#"{
    "openapi": "3.1.0",
    "info": { "title": "Order API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Zebra": {
          "type": "object",
          "properties": {
            "zeta": { "type": "string" },
            "alpha": { "type": "string" }
          }
        },
        "Alpha": {
          "type": "string",
          "enum": ["third", "first", "second"]
        },
        "Beta": {
          "type": "object",
          "properties": {
            "second": { "type": "string" }
          }
        },
        "UniqueNames": {
          "type": "array",
          "uniqueItems": true,
          "items": { "type": "string" }
        },
        "Labels": {
          "type": "object",
          "additionalProperties": { "type": "string" }
        }
      }
    }
  }"#;

  let orchestrator = make_orchestrator(parse_spec(spec_json), true);
  let output = generate_types(&orchestrator, "order.json");

  assert_patterns_in_order(&output.code, &["pub struct Zebra", "pub enum Alpha", "pub struct Beta"]);
  assert_patterns_in_order(&output.code, &["pub zeta", "pub alpha"]);
  assert_patterns_in_order(&output.code, &["Third", "First", "Second"]);
  assert_contains(
    &output.code,
    "pub type UniqueNames = indexmap::IndexSet<String>;",
    "uniqueItems arrays should preserve insertion order",
  );
  assert_contains(
    &output.code,
    "indexmap::IndexMap<String, String>",
    "additionalProperties maps should preserve insertion order",
  );
}

#[test]
fn test_customization_generates_serde_as_attributes() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Test API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Frappe": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "created_at": { "type": "string", "format": "date-time" },
            "updated_at": { "type": "string", "format": "date-time" }
          },
          "required": ["id", "created_at"]
        }
      }
    }
  }"#;

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    "#[serde_with::serde_as]",
    "Struct should have #[serde_with::serde_as] outer attribute",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDateTime")]"#,
    "required field should have serde_as attribute with custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "Option<crate::MyDateTime>")]"#,
    "optional field should have serde_as attribute wrapped in Option",
  );
}

fn assert_patterns_in_order(code: &str, patterns: &[&str]) {
  let mut last = 0;
  for pattern in patterns {
    let next = code[last..]
      .find(pattern)
      .map_or_else(|| panic!("missing ordered pattern '{pattern}'"), |idx| last + idx);
    assert!(next >= last, "pattern '{pattern}' appeared out of order");
    last = next + pattern.len();
  }
}

#[test]
fn test_customization_for_multiple_types() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Cardigan": {
          "type": "object",
          "properties": {
            "id": { "type": "string", "format": "uuid" },
            "created_at": { "type": "string", "format": "date-time" },
            "birth_date": { "type": "string", "format": "date" }
          },
          "required": ["id", "created_at", "birth_date"]
        }
      }
    }
  }"#;

  let customizations = HashMap::from([
    ("date_time".to_string(), "crate::MyDateTime".to_string()),
    ("date".to_string(), "crate::MyDate".to_string()),
    ("uuid".to_string(), "crate::MyUuid".to_string()),
  ]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDateTime")]"#,
    "date-time field should have custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyDate")]"#,
    "date field should have custom type",
  );
  assert_contains(
    &output.code,
    r#"#[serde_as(as = "crate::MyUuid")]"#,
    "uuid field should have custom type",
  );
}

#[test]
fn test_customization_for_array_types() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "WaddleLine": {
          "type": "object",
          "properties": {
            "toebeans": {
              "type": "array",
              "items": { "type": "string", "format": "date-time" }
            }
          },
          "required": ["toebeans"]
        }
      }
    }
  }"#;

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let orchestrator = make_orchestrator_with_customizations(parse_spec(spec_json), true, customizations);
  let output = generate_types(&orchestrator, "test.json");

  assert_contains(
    &output.code,
    r#"#[serde_as(as = "Vec<crate::MyDateTime>")]"#,
    "array field should have serde_as with Vec wrapper",
  );
}

#[test]
fn test_no_customization_no_serde_as() {
  let spec_json = r#"{
    "openapi": "3.0.0",
    "info": { "title": "Pembroke API", "version": "1.0.0" },
    "paths": {},
    "components": {
      "schemas": {
        "Frappe": {
          "type": "object",
          "properties": {
            "id": { "type": "string" },
            "created_at": { "type": "string", "format": "date-time" }
          },
          "required": ["id", "created_at"]
        }
      }
    }
  }"#;

  let orchestrator = make_orchestrator(parse_spec(spec_json), true);
  let output = generate_types(&orchestrator, "test.json");
  assert_not_contains(
    &output.code,
    "#[serde_as(as =",
    "code should not contain serde_as field attribute without customizations",
  );
  assert!(
    !output.code.contains("#[serde_with::serde_as]") || !output.code.contains("Frappe"),
    "Frappe struct should not have serde_as outer attribute without customizations"
  );
}
