use http::Method;

use crate::generator::{
  ast::{
    EnumVariantToken, MethodKind, OperationInfo, OperationKind, ParsedPath, ResponseEnumDef, ResponseMediaType,
    ResponsePayload, ResponseUnionDef, ResponseVariant, RustType, SerdeMode, StatusCodeToken, StructDef, StructKind,
    StructToken, TypeRef,
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

fn param(response: Option<&TypeRef>) -> Option<String> {
  response.map(TypeRef::to_rust_type)
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
