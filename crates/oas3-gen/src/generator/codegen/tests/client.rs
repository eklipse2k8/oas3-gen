use http::Method;
use quote::ToTokens;

use crate::generator::{
  ast::{
    ContentCategory, EnumToken, FieldDef, FieldNameToken, OperationInfo, OperationKind, OperationParameter,
    ParameterLocation, ParsedPath, PathSegment, RustPrimitive, RustType, StructDef, StructKind, StructToken, TypeRef,
  },
  codegen::{
    Visibility,
    client::{ClientGenerator, ClientOperationMethod},
    metadata::CodeMetadata,
  },
};

#[derive(Default)]
struct TestOperation<'a> {
  summary: Option<&'a str>,
  description: Option<&'a str>,
  response_content_category: Option<ContentCategory>,
  response_enum: Option<&'a str>,
}

impl TestOperation<'_> {
  fn build(self) -> OperationInfo {
    OperationInfo {
      stable_id: "test_operation".to_string(),
      operation_id: "testOperation".to_string(),
      method: Method::GET,
      path: ParsedPath(vec![PathSegment::Literal("test".to_string())]),
      path_template: "/test".to_string(),
      kind: OperationKind::Http,
      summary: self.summary.map(String::from),
      description: self.description.map(String::from),
      request_type: Some(StructToken::new("TestRequest")),
      response_type: Some("TestResponse".to_string()),
      response_enum: self.response_enum.map(EnumToken::new),
      response_content_category: self.response_content_category.unwrap_or(ContentCategory::Json),
      success_response_types: vec![],
      error_response_types: vec![],
      warnings: vec![],
      parameters: vec![],
      body: None,
    }
  }
}

#[test]
fn test_build_doc_attributes() {
  struct Case {
    summary: Option<&'static str>,
    description: Option<&'static str>,
    expected_contains: Vec<&'static str>,
    expected_missing: Vec<&'static str>,
  }

  let cases = [
    Case {
      summary: Some("Test summary"),
      description: None,
      expected_contains: vec!["Test summary", "GET /test"],
      expected_missing: vec!["Test description"],
    },
    Case {
      summary: None,
      description: Some("Test description"),
      expected_contains: vec!["Test description", "GET /test"],
      expected_missing: vec![],
    },
    Case {
      summary: Some("Test summary"),
      description: Some("Test description"),
      expected_contains: vec!["Test summary", "Test description", "GET /test"],
      expected_missing: vec![],
    },
    Case {
      summary: Some("Test summary"),
      description: Some("Line 1\nLine 2\nLine 3"),
      expected_contains: vec!["Line 1", "Line 2", "Line 3"],
      expected_missing: vec![],
    },
    Case {
      summary: None,
      description: None,
      expected_contains: vec!["GET /test"],
      expected_missing: vec![],
    },
  ];

  for case in cases {
    let label = format!("summary={:?}, description={:?}", case.summary, case.description);
    let operation = TestOperation {
      summary: case.summary,
      description: case.description,
      ..Default::default()
    }
    .build();
    let doc_attrs = ClientOperationMethod::build_doc_attributes(&operation);
    let output = doc_attrs.to_string();

    for expected in case.expected_contains {
      assert!(output.contains(expected), "{label}: expected to contain '{expected}'");
    }
    for missing in case.expected_missing {
      assert!(
        !output.contains(missing),
        "{label}: expected NOT to contain '{missing}'"
      );
    }
  }

  let operation = TestOperation {
    summary: Some("Test summary"),
    description: Some("Test description"),
    ..Default::default()
  }
  .build();
  let doc_attrs = ClientOperationMethod::build_doc_attributes(&operation);
  let output = doc_attrs.to_string();
  let summary_pos = output.find("Test summary").unwrap();
  let description_pos = output.find("Test description").unwrap();
  let signature_pos = output.find("GET /test").unwrap();
  assert!(summary_pos < description_pos, "summary should come before description");
  assert!(
    description_pos < signature_pos,
    "description should come before signature"
  );
}

#[test]
fn test_response_handling_content_categories() {
  struct Case {
    category: ContentCategory,
    expected_return_ty: &'static str,
    expected_contains: Vec<&'static str>,
  }

  let cases = [
    Case {
      category: ContentCategory::Json,
      expected_return_ty: "TestResponse",
      expected_contains: vec!["json", "TestResponse"],
    },
    Case {
      category: ContentCategory::Text,
      expected_return_ty: "String",
      expected_contains: vec!["text"],
    },
    Case {
      category: ContentCategory::Binary,
      expected_return_ty: "reqwest :: Response",
      expected_contains: vec!["Ok (response)"],
    },
    Case {
      category: ContentCategory::Xml,
      expected_return_ty: "reqwest :: Response",
      expected_contains: vec!["Ok (response)"],
    },
    Case {
      category: ContentCategory::FormUrlEncoded,
      expected_return_ty: "reqwest :: Response",
      expected_contains: vec!["Ok (response)"],
    },
    Case {
      category: ContentCategory::Multipart,
      expected_return_ty: "reqwest :: Response",
      expected_contains: vec!["Ok (response)"],
    },
  ];

  for case in cases {
    let label = format!("category={:?}", case.category);
    let operation = TestOperation {
      response_content_category: Some(case.category),
      ..Default::default()
    }
    .build();
    let method = ClientOperationMethod::try_from_operation(&operation, &[], Visibility::Public).unwrap();

    let return_ty_str = method.response_handling.success_type.to_string();
    let response_str = method.response_handling.parse_body.to_string();

    assert_eq!(return_ty_str, case.expected_return_ty, "{label}: return type mismatch");
    for expected in case.expected_contains {
      assert!(
        response_str.contains(expected),
        "{label}: expected response to contain '{expected}'"
      );
    }
  }
}

#[test]
fn test_response_handling_with_response_enum() {
  let operation = TestOperation {
    response_enum: Some("TestResponseEnum"),
    ..Default::default()
  }
  .build();
  let method = ClientOperationMethod::try_from_operation(&operation, &[], Visibility::Public).unwrap();

  let success_type_str = method.response_handling.success_type.to_string();
  let parse_body_str = method.response_handling.parse_body.to_string();

  assert!(
    success_type_str.contains("TestResponseEnum"),
    "success type should contain TestResponseEnum"
  );
  assert!(
    parse_body_str.contains("parse_response"),
    "parse_body should use parse_response"
  );
}

#[test]
fn test_multipart_generation() {
  let binary_field = FieldDef {
    name: FieldNameToken::new("file"),
    rust_type: TypeRef {
      base_type: RustPrimitive::Bytes,
      is_array: false,
      nullable: false,
      boxed: false,
      unique_items: false,
    },
    ..Default::default()
  };

  let text_field = FieldDef {
    name: FieldNameToken::new("description"),
    rust_type: TypeRef {
      base_type: RustPrimitive::String,
      is_array: false,
      nullable: false,
      boxed: false,
      unique_items: false,
    },
    ..Default::default()
  };

  let body_struct = StructDef {
    name: StructToken::new("MultipartBody"),
    fields: vec![binary_field, text_field],
    docs: vec![],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
    ..Default::default()
  };

  let request_struct = StructDef {
    name: StructToken::new("UploadRequest"),
    fields: vec![FieldDef {
      name: FieldNameToken::new("body"),
      rust_type: TypeRef {
        base_type: RustPrimitive::Custom("MultipartBody".into()),
        is_array: false,
        nullable: false,
        boxed: false,
        unique_items: false,
      },
      ..Default::default()
    }],
    docs: vec![],
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
    ..Default::default()
  };

  let make_operation = |request_type: &str| OperationInfo {
    stable_id: format!("{request_type}_operation"),
    operation_id: format!("{request_type}Operation"),
    method: Method::GET,
    path: ParsedPath(vec![PathSegment::Literal("test".to_string())]),
    path_template: "/test".to_string(),
    kind: OperationKind::Http,
    summary: None,
    description: None,
    request_type: Some(StructToken::new(request_type)),
    response_type: Some("TestResponse".to_string()),
    response_enum: None,
    response_content_category: ContentCategory::Json,
    success_response_types: vec![],
    error_response_types: vec![],
    warnings: vec![],
    parameters: vec![],
    body: None,
  };

  let field_ident = FieldNameToken::new("body");

  let rust_types = vec![RustType::Struct(request_struct), RustType::Struct(body_struct)];
  let strict_operation = make_operation("UploadRequest");
  let strict_code =
    ClientOperationMethod::build_multipart_body(&field_ident, false, &strict_operation, &rust_types).to_string();

  assert!(
    strict_code.contains("Part :: bytes"),
    "strict: should use Part::bytes for binary"
  );
  assert!(
    strict_code.contains("Part :: text"),
    "strict: should use Part::text for text"
  );
  assert!(
    strict_code.contains("form . part (\"file\""),
    "strict: should have file part"
  );
  assert!(
    strict_code.contains("form . part (\"description\""),
    "strict: should have description part"
  );
  assert!(
    !strict_code.contains("serde_json :: to_value"),
    "strict: should NOT use fallback"
  );

  let fallback_operation = make_operation("UnknownRequest");
  let fallback_code =
    ClientOperationMethod::build_multipart_body(&field_ident, false, &fallback_operation, &[]).to_string();

  assert!(
    fallback_code.contains("serde_json :: to_value"),
    "fallback: should use serde_json"
  );
  assert!(fallback_code.contains("form . text"), "fallback: should use form.text");
  assert!(
    !fallback_code.contains("Part :: bytes"),
    "fallback: should NOT use Part::bytes"
  );
}

#[test]
fn test_client_filters_webhook_operations() {
  let http_operation = OperationInfo {
    stable_id: "list_pets".to_string(),
    operation_id: "listPets".to_string(),
    method: Method::GET,
    path: ParsedPath(vec![PathSegment::Literal("pets".to_string())]),
    path_template: "/pets".to_string(),
    kind: OperationKind::Http,
    summary: Some("List all pets".to_string()),
    description: None,
    request_type: Some(StructToken::new("ListPetsRequest")),
    response_type: Some("Vec<Pet>".to_string()),
    response_enum: None,
    response_content_category: ContentCategory::Json,
    success_response_types: vec![],
    error_response_types: vec![],
    warnings: vec![],
    parameters: vec![OperationParameter {
      original_name: "X-Custom-Header".to_string(),
      rust_field: FieldNameToken::new("x_custom_header"),
      location: ParameterLocation::Header,
      required: false,
      rust_type: TypeRef {
        base_type: RustPrimitive::String,
        is_array: false,
        nullable: true,
        boxed: false,
        unique_items: false,
      },
    }],
    body: None,
  };

  let webhook_operation = OperationInfo {
    stable_id: "pet_added_hook".to_string(),
    operation_id: "petAddedHook".to_string(),
    method: Method::POST,
    path: ParsedPath(vec![
      PathSegment::Literal("webhooks".to_string()),
      PathSegment::Literal("petAdded".to_string()),
    ]),
    path_template: "webhooks/petAdded".to_string(),
    kind: OperationKind::Webhook,
    summary: Some("Pet added webhook".to_string()),
    description: None,
    request_type: Some(StructToken::new("PetAddedHookRequest")),
    response_type: Some("WebhookResponse".to_string()),
    response_enum: None,
    response_content_category: ContentCategory::Json,
    success_response_types: vec![],
    error_response_types: vec![],
    warnings: vec![],
    parameters: vec![OperationParameter {
      original_name: "X-Webhook-Secret".to_string(),
      rust_field: FieldNameToken::new("x_webhook_secret"),
      location: ParameterLocation::Header,
      required: true,
      rust_type: TypeRef {
        base_type: RustPrimitive::String,
        is_array: false,
        nullable: false,
        boxed: false,
        unique_items: false,
      },
    }],
    body: None,
  };

  let operations = vec![http_operation, webhook_operation];
  let metadata = CodeMetadata {
    title: "PetStore".to_string(),
    base_url: "https://api.example.com".to_string(),
    version: "1.0.0".to_string(),
    description: None,
  };

  let generator = ClientGenerator::new(&metadata, &operations, &[], Visibility::Public);
  let output = generator.to_token_stream().to_string();

  // HTTP operation should generate a client method
  assert!(
    output.contains("list_pets"),
    "HTTP operation method should be generated"
  );

  // Webhook operation should NOT generate a client method
  assert!(
    !output.contains("pet_added_hook"),
    "Webhook operation method should NOT be generated"
  );

  // HTTP header constants should be generated
  assert!(
    output.contains("X_CUSTOM_HEADER") || output.contains("X-Custom-Header"),
    "HTTP header constant should be generated"
  );

  // Webhook header constants should NOT be generated
  assert!(
    !output.contains("X_WEBHOOK_SECRET") && !output.contains("X-Webhook-Secret"),
    "Webhook header constant should NOT be generated"
  );
}

#[test]
fn test_path_segments_static_path() {
  let segments = ParsedPath(vec![PathSegment::Literal("pets".to_string())]);
  let output = segments
    .0
    .iter()
    .map(|s| s.to_token_stream().to_string())
    .collect::<String>();

  assert!(
    output.contains("push") && output.contains("pets"),
    "should push 'pets' segment: {output}"
  );
  assert_eq!(segments.0.len(), 1, "should have exactly one segment");
}

#[test]
fn test_path_segments_single_param() {
  let segments = ParsedPath(vec![
    PathSegment::Literal("pets".to_string()),
    PathSegment::Param(FieldNameToken::new("pet_id")),
  ]);
  let output = segments
    .0
    .iter()
    .map(|s| s.to_token_stream().to_string())
    .collect::<String>();

  assert!(output.contains("pets"), "should push 'pets' literal: {output}");
  assert!(output.contains("pet_id"), "should reference path param: {output}");
  assert_eq!(segments.0.len(), 2, "should have two segments");
}

#[test]
fn test_path_segments_multiple_params() {
  let segments = ParsedPath(vec![
    PathSegment::Literal("users".to_string()),
    PathSegment::Param(FieldNameToken::new("user_id")),
    PathSegment::Literal("posts".to_string()),
    PathSegment::Param(FieldNameToken::new("post_id")),
  ]);
  let output = segments
    .0
    .iter()
    .map(|s| s.to_token_stream().to_string())
    .collect::<String>();

  assert!(output.contains("users"), "should push 'users': {output}");
  assert!(output.contains("posts"), "should push 'posts': {output}");
  assert!(output.contains("user_id"), "should reference user_id: {output}");
  assert!(output.contains("post_id"), "should reference post_id: {output}");
  assert_eq!(segments.0.len(), 4, "should have four segments");
}

#[test]
fn test_path_segments_mixed_segment() {
  let segments = ParsedPath(vec![
    PathSegment::Literal("api".to_string()),
    PathSegment::Mixed {
      format: "v{}.json".to_string(),
      params: vec![FieldNameToken::new("version")],
    },
  ]);
  let output = segments
    .0
    .iter()
    .map(|s| s.to_token_stream().to_string())
    .collect::<String>();

  assert!(output.contains("api"), "should push 'api': {output}");
  assert!(
    output.contains("format"),
    "should use format! for mixed segment: {output}"
  );
  assert!(
    output.contains("v{}.json"),
    "should have correct format string: {output}"
  );
  assert_eq!(segments.0.len(), 2, "should have two segments");
}

#[test]
fn test_url_path_segments_encoding() {
  use reqwest::Url;

  let mut url = Url::parse("http://example.com").unwrap();

  url
    .path_segments_mut()
    .expect("valid URL")
    .clear()
    .push("pets")
    .push("cat/dog");

  assert_eq!(url.path(), "/pets/cat%2Fdog", "slash should be encoded as %2F");

  url
    .path_segments_mut()
    .expect("valid URL")
    .clear()
    .push("pets")
    .push("hello world");

  assert_eq!(url.path(), "/pets/hello%20world", "space should be encoded as %20");

  url
    .path_segments_mut()
    .expect("valid URL")
    .clear()
    .push("pets")
    .push("100%");

  assert_eq!(url.path(), "/pets/100%25", "percent should be encoded as %25");

  url
    .path_segments_mut()
    .expect("valid URL")
    .clear()
    .push("pets")
    .push("a?b#c");

  assert_eq!(url.path(), "/pets/a%3Fb%23c", "query/fragment chars should be encoded");

  url
    .path_segments_mut()
    .expect("valid URL")
    .clear()
    .push("pets")
    .push("café");

  assert_eq!(url.path(), "/pets/caf%C3%A9", "unicode should be percent-encoded");
}
