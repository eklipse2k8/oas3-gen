use http::Method;

use crate::generator::{
  ast::{
    EnumDef, EnumToken, EnumVariantToken, FieldDef, FieldNameToken, MethodKind, OperationInfo, OperationKind,
    ParsedPath, ResponseEnumDef, ResponseMediaType, ResponseParam, ResponsePayload, ResponseUnionDef, ResponseVariant,
    RustType, SerdeMode, StatusCodeToken, StructDef, StructKind, StructToken, TypeRef,
  },
  converter::GenerationTarget,
  postprocess::response::ResponseProcessor,
};

fn variant(status: StatusCodeToken, body: Option<&str>) -> ResponseVariant {
  let schema = body.map(TypeRef::new);
  ResponseVariant::builder()
    .status_code(status)
    .media_types(vec![ResponseMediaType::with_schema("application/json", schema.clone())])
    .maybe_schema_type(schema)
    .build()
}

fn header(name: &str, rust_type: &str, required: bool) -> FieldDef {
  let rust_type = TypeRef::new(rust_type);
  FieldDef::builder()
    .name(FieldNameToken::from_raw(name))
    .rust_type(if required { rust_type } else { rust_type.with_option() })
    .original_name(name)
    .build()
}

fn with_headers(variant: ResponseVariant, headers: Vec<FieldDef>) -> ResponseVariant {
  ResponseVariant { headers, ..variant }
}

fn operation(id: &str, variants: Option<Vec<ResponseVariant>>) -> OperationInfo {
  OperationInfo::builder()
    .stable_id(id)
    .operation_id(id)
    .method(Method::GET)
    .path(ParsedPath {
      segments: vec![],
      query_string: None,
    })
    .kind(OperationKind::Http)
    .request_type(StructToken::from_raw(format!("{id}_request")))
    .maybe_response_variants(variants)
    .build()
}

fn request_struct(id: &str) -> RustType {
  RustType::Struct(StructDef {
    name: StructToken::from_raw(format!("{id}_request")),
    kind: StructKind::OperationRequest,
    ..Default::default()
  })
}

fn assemble(
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  target: GenerationTarget,
) -> (Vec<RustType>, Vec<OperationInfo>) {
  ResponseProcessor::new(types, operations, target).process()
}

fn response_enum(types: &[RustType]) -> &ResponseEnumDef {
  types
    .iter()
    .find_map(|t| match t {
      RustType::ResponseEnum(def) => Some(def),
      _ => None,
    })
    .expect("response enum not generated")
}

fn unions(types: &[RustType]) -> Vec<&ResponseUnionDef> {
  types
    .iter()
    .filter_map(|t| match t {
      RustType::ResponseUnion(def) => Some(def),
      _ => None,
    })
    .collect()
}

fn param(response: Option<&ResponseParam>) -> Option<String> {
  response.map(|param| param.body.to_rust_type())
}

fn headers_of(response: Option<&ResponseParam>) -> Option<String> {
  response
    .and_then(|param| param.headers.as_ref())
    .map(|headers| headers.name.to_string())
}

fn header_structs(types: &[RustType]) -> Vec<&StructDef> {
  types
    .iter()
    .filter_map(|t| match t {
      RustType::Struct(def) if def.kind == StructKind::ResponseHeaders => Some(def),
      _ => None,
    })
    .collect()
}

#[test]
fn test_response_enum_collects_status_codes_across_operations() {
  let types = vec![request_struct("get_pet"), request_struct("create_pet")];
  let operations = vec![
    operation(
      "get_pet",
      Some(vec![
        variant(StatusCodeToken::Ok200, Some("Pet")),
        variant(StatusCodeToken::NotFound404, Some("Error")),
      ]),
    ),
    operation(
      "create_pet",
      Some(vec![
        variant(StatusCodeToken::Created201, None),
        variant(StatusCodeToken::Default, Some("Error")),
      ]),
    ),
  ];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);
  let response_enum = response_enum(&types);

  let expected = [
    ("Ok", ResponsePayload::Value, false),
    ("Created", ResponsePayload::None, false),
    ("NotFound", ResponsePayload::Failure, false),
    ("Unknown", ResponsePayload::Failure, true),
    ("Other", ResponsePayload::Raw, true),
  ];
  let actual = response_enum
    .variants
    .iter()
    .map(|v| (v.name.to_string(), v.payload, v.status_code.carries_status()))
    .collect::<Vec<_>>();
  for ((name, payload, carries_status), actual) in expected.iter().zip(&actual) {
    assert_eq!(
      (name.to_string(), *payload, *carries_status),
      *actual,
      "variant mismatch in {actual:?}"
    );
  }
  assert_eq!(actual.len(), expected.len(), "unexpected variant count: {actual:?}");
  assert_eq!(response_enum.name, "ApiResponse");
  assert!(response_enum.has_param(ResponsePayload::Value) && response_enum.has_param(ResponsePayload::Failure));

  let responses = operations
    .iter()
    .map(|op| {
      let response = op.response.as_ref().expect("response resolved");
      (
        op.stable_id.clone(),
        param(response.value.as_ref()),
        param(response.failure.as_ref()),
      )
    })
    .collect::<Vec<_>>();
  assert_eq!(
    responses,
    vec![
      (
        "get_pet".to_string(),
        Some("Pet".to_string()),
        Some("Error".to_string())
      ),
      (
        "create_pet".to_string(),
        Some("()".to_string()),
        Some("Error".to_string())
      ),
    ]
  );
  assert!(unions(&types).is_empty(), "no union needed for single body types");
}

#[test]
fn test_parse_method_attached_only_for_client_target() {
  let cases = [(GenerationTarget::Client, 1), (GenerationTarget::Server, 0)];

  for (target, expected_methods) in cases {
    let types = vec![request_struct("get_pet")];
    let operations = vec![operation(
      "get_pet",
      Some(vec![variant(StatusCodeToken::Ok200, Some("Pet"))]),
    )];

    let (types, _) = assemble(types, operations, target);
    let request = types
      .iter()
      .find_map(|t| match t {
        RustType::Struct(def) if def.name == "GetPetRequest" => Some(def),
        _ => None,
      })
      .expect("request struct kept");

    let parse_methods = request
      .methods
      .iter()
      .filter(|m| matches!(m.kind, MethodKind::ParseResponse { .. }))
      .count();
    assert_eq!(parse_methods, expected_methods, "parse method count for {target:?}");
  }
}

#[test]
fn test_mixed_body_presence_makes_parameter_optional() {
  let types = vec![request_struct("start_job"), request_struct("poll_job")];
  let operations = vec![
    operation(
      "start_job",
      Some(vec![
        variant(StatusCodeToken::Ok200, Some("Job")),
        variant(StatusCodeToken::Accepted202, None),
      ]),
    ),
    operation(
      "poll_job",
      Some(vec![variant(StatusCodeToken::Accepted202, Some("Progress"))]),
    ),
  ];

  let (_, operations) = assemble(types, operations, GenerationTarget::Client);

  let start_job = &operations[0];
  let response = start_job.response.as_ref().unwrap();
  assert_eq!(param(response.value.as_ref()).as_deref(), Some("Option<Job>"));

  let slots = start_job
    .response_variants
    .as_ref()
    .unwrap()
    .iter()
    .map(|v| (v.status_code, v.mapping.payload))
    .collect::<Vec<_>>();
  assert_eq!(
    slots,
    vec![
      (StatusCodeToken::Ok200, ResponsePayload::Value),
      (StatusCodeToken::Accepted202, ResponsePayload::Value),
    ]
  );

  let poll_job = operations[1].response.as_ref().unwrap();
  assert_eq!(param(poll_job.value.as_ref()).as_deref(), Some("Progress"));
}

#[test]
fn test_several_body_types_share_one_payload_union() {
  let types = vec![request_struct("get_user"), request_struct("get_team")];
  let operations = vec![
    operation(
      "get_user",
      Some(vec![
        variant(StatusCodeToken::Ok200, Some("User")),
        variant(StatusCodeToken::UnprocessableEntity422, Some("ValidationError")),
        variant(StatusCodeToken::NotFound404, Some("BasicError")),
      ]),
    ),
    operation(
      "get_team",
      Some(vec![
        variant(StatusCodeToken::Ok200, Some("Team")),
        variant(StatusCodeToken::NotFound404, Some("BasicError")),
        variant(StatusCodeToken::UnprocessableEntity422, Some("ValidationError")),
      ]),
    ),
  ];

  let (types, operations) = assemble(types, operations, GenerationTarget::Server);

  let unions = unions(&types);
  assert_eq!(unions.len(), 1, "identical member sets share one union");
  let union = unions[0];
  assert_eq!(union.name, "BasicErrorOrValidationError");
  assert_eq!(union.serde_mode, SerdeMode::SerializeOnly);
  assert_eq!(
    union
      .variants
      .iter()
      .map(|v| (v.name.to_string(), v.rust_type.to_rust_type()))
      .collect::<Vec<_>>(),
    vec![
      ("BasicError".to_string(), "BasicError".to_string()),
      ("ValidationError".to_string(), "ValidationError".to_string()),
    ]
  );

  for op in &operations {
    let response = op.response.as_ref().unwrap();
    assert_eq!(
      param(response.failure.as_ref()).as_deref(),
      Some("BasicErrorOrValidationError"),
      "error parameter for {}",
      op.stable_id
    );

    let not_found = op
      .response_variants
      .as_ref()
      .unwrap()
      .iter()
      .find(|v| v.status_code == StatusCodeToken::NotFound404)
      .unwrap();
    assert_eq!(
      not_found.mapping.union_variant.as_ref().map(EnumVariantToken::as_str),
      Some("BasicError")
    );
  }
}

#[test]
fn test_no_bodies_means_no_type_parameters() {
  let types = vec![request_struct("ping")];
  let operations = vec![operation(
    "ping",
    Some(vec![variant(StatusCodeToken::NoContent204, None)]),
  )];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);
  let response_enum = response_enum(&types);

  let names = response_enum
    .variants
    .iter()
    .map(|v| v.name.to_string())
    .collect::<Vec<_>>();
  assert_eq!(names, vec!["NoContent", "Other"]);
  assert!(
    !response_enum.has_param(ResponsePayload::Value) && !response_enum.has_param(ResponsePayload::Failure),
    "no body anywhere means no type parameters"
  );

  let response = operations[0].response.as_ref().unwrap();
  assert!(response.value.is_none() && response.failure.is_none());
}

#[test]
fn test_response_enum_name_avoids_schema_collision() {
  let types = vec![
    request_struct("get_pet"),
    RustType::Struct(StructDef {
      name: StructToken::new("ApiResponse"),
      kind: StructKind::Schema,
      ..Default::default()
    }),
  ];
  let operations = vec![operation(
    "get_pet",
    Some(vec![variant(StatusCodeToken::Ok200, Some("ApiResponse"))]),
  )];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);

  assert_eq!(response_enum(&types).name, "ApiResponseType");
  assert_eq!(operations[0].response.as_ref().unwrap().name, "ApiResponseType");
}

#[test]
fn test_no_declared_responses_means_no_response_enum() {
  let types = vec![request_struct("fire_and_forget")];
  let operations = vec![operation("fire_and_forget", None)];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);

  assert!(
    !types.iter().any(|t| matches!(t, RustType::ResponseEnum(_))),
    "no response enum without responses"
  );
  assert!(operations[0].response.is_none());
}

#[test]
fn test_headers_merge_across_a_status_class() {
  let types = vec![request_struct("list_pets")];
  let operations = vec![operation(
    "list_pets",
    Some(vec![
      with_headers(
        variant(StatusCodeToken::Ok200, Some("Pets")),
        vec![header("Link", "String", true), header("X-Total", "i64", true)],
      ),
      with_headers(
        variant(StatusCodeToken::Created201, None),
        vec![header("Link", "String", true)],
      ),
      variant(StatusCodeToken::NotFound404, Some("Error")),
    ]),
  )];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);
  let response = operations[0].response.as_ref().expect("response not resolved");

  assert_eq!(param(response.value.as_ref()).as_deref(), Some("Option<Pets>"));
  assert_eq!(
    headers_of(response.value.as_ref()).as_deref(),
    Some("LinkAndXTotalHeaders"),
    "success statuses share one headers struct"
  );
  assert_eq!(
    headers_of(response.failure.as_ref()),
    None,
    "failure statuses declare no headers"
  );

  let structs = header_structs(&types);
  let [headers] = structs.as_slice() else {
    panic!("expected one headers struct, got {structs:?}");
  };
  let fields = headers
    .fields
    .iter()
    .map(|f| (f.name.to_string(), f.rust_type.to_rust_type()))
    .collect::<Vec<_>>();
  let expected = [("link", "String"), ("x_total", "Option<i64>")].map(|(name, ty)| (name.to_string(), ty.to_string()));
  assert_eq!(
    fields, expected,
    "a header is required only when every status in the class requires it"
  );

  let created = response_enum(&types)
    .variants
    .iter()
    .find(|v| v.status_code == StatusCodeToken::Created201)
    .expect("Created variant not generated");
  assert_eq!(
    created.payload,
    ResponsePayload::Value,
    "a bodiless status whose class declares headers carries a payload"
  );
}

#[test]
fn test_header_structs_are_shared_by_header_set() {
  let link = |rust_type| vec![header("Link", rust_type, false)];
  let types = ["list_pets", "list_cats", "count_pets", "get_pet"]
    .into_iter()
    .map(request_struct)
    .collect::<Vec<_>>();
  let operations = vec![
    operation(
      "list_pets",
      Some(vec![with_headers(
        variant(StatusCodeToken::Ok200, Some("Pets")),
        link("String"),
      )]),
    ),
    operation(
      "list_cats",
      Some(vec![with_headers(
        variant(StatusCodeToken::Ok200, Some("Cats")),
        link("String"),
      )]),
    ),
    operation(
      "count_pets",
      Some(vec![with_headers(
        variant(StatusCodeToken::Ok200, Some("i64")),
        link("i64"),
      )]),
    ),
    operation(
      "get_pet",
      Some(vec![
        variant(StatusCodeToken::Ok200, Some("Pet")),
        with_headers(variant(StatusCodeToken::NotFound404, Some("Error")), link("String")),
      ]),
    ),
  ];

  let (types, operations) = assemble(types, operations, GenerationTarget::Client);

  let expected = [
    ("list_pets", Some("LinkHeaders"), None),
    ("list_cats", Some("LinkHeaders"), None),
    ("count_pets", Some("LinkHeaders2"), None),
    ("get_pet", None, Some("LinkHeaders")),
  ];
  for (op, (id, value, failure)) in operations.iter().zip(expected) {
    let response = op.response.as_ref().expect("response not resolved");
    assert_eq!(
      (
        headers_of(response.value.as_ref()).as_deref(),
        headers_of(response.failure.as_ref()).as_deref()
      ),
      (value, failure),
      "headers mismatch for {id}"
    );
  }
  assert_eq!(header_structs(&types).len(), 2, "one struct per distinct header set");
}

#[test]
fn test_enums_parsed_from_response_headers_are_flagged() {
  let enum_type = |name: &str| {
    RustType::Enum(EnumDef {
      name: EnumToken::new(name),
      ..Default::default()
    })
  };
  let types = vec![
    request_struct("get_pet"),
    enum_type("CacheStatus"),
    enum_type("PetKind"),
  ];
  let operations = vec![operation(
    "get_pet",
    Some(vec![with_headers(
      variant(StatusCodeToken::Ok200, Some("Pet")),
      vec![header("x-cache", "CacheStatus", false)],
    )]),
  )];

  let (types, _) = assemble(types, operations, GenerationTarget::Client);

  let flagged = types
    .iter()
    .filter_map(|t| match t {
      RustType::Enum(def) => Some((def.name.to_string(), def.in_response_header)),
      _ => None,
    })
    .collect::<Vec<_>>();
  assert_eq!(
    flagged,
    [("CacheStatus".to_string(), true), ("PetKind".to_string(), false)],
    "only enums read from response headers are flagged"
  );
}

#[test]
fn test_with_headers_wrapper_only_when_headers_are_declared() {
  let schema_named_wrapper = RustType::Struct(StructDef {
    name: StructToken::new("WithHeaders"),
    ..Default::default()
  });
  let headed = || {
    vec![with_headers(
      variant(StatusCodeToken::Ok200, Some("Pet")),
      vec![header("x-next", "String", false)],
    )]
  };
  let cases = [
    (
      "no headers",
      vec![],
      vec![variant(StatusCodeToken::Ok200, Some("Pet"))],
      None,
    ),
    ("headers", vec![], headed(), Some("WithHeaders")),
    (
      "schema already named WithHeaders",
      vec![schema_named_wrapper],
      headed(),
      Some("WithHeaders2"),
    ),
  ];

  for (label, schemas, variants, expected) in cases {
    let types = std::iter::once(request_struct("get_pet"))
      .chain(schemas)
      .collect::<Vec<_>>();
    let (types, operations) = assemble(
      types,
      vec![operation("get_pet", Some(variants))],
      GenerationTarget::Client,
    );
    let response = operations[0].response.as_ref().expect("response not resolved");
    assert_eq!(
      response_enum(&types)
        .with_headers
        .as_ref()
        .map(ToString::to_string)
        .as_deref(),
      expected,
      "{label}: wrapper emitted with the response enum"
    );
    assert_eq!(
      response
        .value
        .as_ref()
        .and_then(|param| param.headers.as_ref())
        .map(|headers| headers.wrapper.to_string())
        .as_deref(),
      expected,
      "{label}: wrapper used by the operation"
    );
  }
}
