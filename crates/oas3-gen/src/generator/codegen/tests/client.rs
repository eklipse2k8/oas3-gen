use std::collections::BTreeSet;

use http::Method;
use quote::format_ident;

use crate::generator::{
  ast::{
    ContentCategory, EnumToken, FieldDef, OperationBody, OperationInfo, RustPrimitive, RustType, StructDef, StructKind,
    TypeRef,
  },
  codegen::client::ClientOperationMethod,
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
      path: "/test".to_string(),
      summary: self.summary.map(String::from),
      description: self.description.map(String::from),
      request_type: Some("TestRequest".to_string()),
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
    let method = ClientOperationMethod::try_from_operation(&operation, &[]).unwrap();

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
  let method = ClientOperationMethod::try_from_operation(&operation, &[]).unwrap();

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
    name: "file".to_string(),
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
    name: "description".to_string(),
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
    name: "MultipartBody".to_string(),
    fields: vec![binary_field, text_field],
    docs: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::RequestBody,
  };

  let request_struct = StructDef {
    name: "UploadRequest".to_string(),
    fields: vec![FieldDef {
      name: "body".to_string(),
      rust_type: TypeRef {
        base_type: RustPrimitive::Custom("MultipartBody".to_string()),
        is_array: false,
        nullable: false,
        boxed: false,
        unique_items: false,
      },
      ..Default::default()
    }],
    docs: vec![],
    derives: BTreeSet::new(),
    serde_attrs: vec![],
    outer_attrs: vec![],
    methods: vec![],
    kind: StructKind::OperationRequest,
  };

  let make_operation = |request_type: &str| OperationInfo {
    stable_id: "upload".to_string(),
    operation_id: "upload".to_string(),
    method: Method::POST,
    path: "/upload".to_string(),
    summary: None,
    description: None,
    request_type: Some(request_type.to_string()),
    response_type: None,
    response_enum: None,
    response_content_category: ContentCategory::Json,
    success_response_types: vec![],
    error_response_types: vec![],
    warnings: vec![],
    parameters: vec![],
    body: Some(OperationBody {
      field_name: "body".to_string(),
      optional: false,
      content_category: ContentCategory::Multipart,
    }),
  };

  let field_ident = format_ident!("body");

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
