use std::collections::BTreeSet;

use http::Method;
use indexmap::IndexMap;

use crate::generator::{
  ast::{
    ContentCategory, Documentation, EnumDef, EnumToken, FieldDef, FieldNameToken, MultipartPartKind, MultipartProperty,
    OperationBody, OperationInfo, OperationKind, ParsedPath, PartMembers, PartRfc6570, PartStyle, PartType, PartValue,
    RustPrimitive, RustType, SerdeAsFieldAttr, SerdeAttribute, SerdeMode, StructDef, StructKind, TypeAliasDef,
    TypeAliasToken, TypeRef,
  },
  postprocess::multipart::MultipartProcessor,
};

fn field(name: &str, rust_type: TypeRef) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::from_raw(name))
    .rust_type(rust_type)
    .build()
}

fn with_serde(mut field: FieldDef, attr: SerdeAttribute) -> FieldDef {
  field.serde_attrs = BTreeSet::from([attr]);
  field
}

fn with_serde_as(mut field: FieldDef, custom_type: &str) -> FieldDef {
  field.serde_as_attr = Some(SerdeAsFieldAttr::CustomOverride {
    custom_type: custom_type.to_string(),
    optional: field.rust_type.nullable,
    is_array: field.rust_type.is_array,
  });
  field
}

fn schema_struct(name: &str, fields: Vec<FieldDef>, serde_mode: SerdeMode) -> RustType {
  RustType::Struct(
    StructDef::builder()
      .name(name)
      .fields(fields)
      .kind(StructKind::Schema)
      .serde_mode(serde_mode)
      .build(),
  )
}

fn value_enum(name: &str, scalar_repr: Option<RustPrimitive>) -> RustType {
  RustType::Enum(EnumDef {
    name: EnumToken::new(name),
    generate_display: true,
    scalar_repr,
    ..Default::default()
  })
}

fn operation(
  body_type: &str,
  category: ContentCategory,
  properties: IndexMap<String, MultipartProperty>,
) -> OperationInfo {
  OperationInfo::builder()
    .stable_id("submit_form")
    .operation_id("submitForm")
    .method(Method::POST)
    .path(ParsedPath::default())
    .kind(OperationKind::Http)
    .body(
      OperationBody::builder()
        .field_name(FieldNameToken::new("body"))
        .body_type(TypeRef::new(body_type))
        .content_category(category)
        .multipart_properties(properties)
        .build(),
    )
    .build()
}

fn part_type(rust_type: TypeRef, value: PartValue) -> PartType {
  PartType { rust_type, value }
}

fn styled(style: PartStyle, explode: bool) -> MultipartProperty {
  MultipartProperty {
    rfc6570: Some(PartRfc6570 { style, explode }),
    ..Default::default()
  }
}

#[test]
fn test_multipart_fields_classify_every_part_kind() {
  let form = schema_struct(
    "UploadForm",
    vec![
      field("name", TypeRef::new(RustPrimitive::String)),
      with_serde(
        field("file_name", TypeRef::new(RustPrimitive::String).with_option()),
        SerdeAttribute::Rename("fileName".to_string()),
      ),
      field("avatar", TypeRef::new(RustPrimitive::Value)),
      field("metadata", TypeRef::new(RustPrimitive::Value).with_option()),
      field("files", TypeRef::new(RustPrimitive::Bytes).with_vec()),
      with_serde_as(
        field("encoded", TypeRef::new(RustPrimitive::Bytes)),
        "serde_with::base64::Base64",
      ),
      field("quality", TypeRef::new("Quality").with_option()),
      field("level", TypeRef::new("Level")),
      field("address", TypeRef::new("Address")),
      field("labels", TypeRef::new("Labels")),
      field("alias", TypeRef::new("Nickname")),
      field("created_at", TypeRef::new(RustPrimitive::DateTime)),
      field("elapsed", TypeRef::new(RustPrimitive::Duration)),
      with_serde_as(
        field("updated_at", TypeRef::new(RustPrimitive::DateTime)),
        "crate::Stamp",
      ),
      field("count", TypeRef::new(RustPrimitive::I64)),
      field("ids", TypeRef::new(RustPrimitive::I64).with_vec()),
      field("filter", TypeRef::new("Filter").with_option()),
      field(
        "options",
        TypeRef::map("indexmap::IndexMap", &TypeRef::new(RustPrimitive::I64)),
      ),
      field("note", TypeRef::new(RustPrimitive::String)),
      with_serde(
        field("hidden", TypeRef::new(RustPrimitive::String)),
        SerdeAttribute::Skip,
      ),
      FieldDef::builder()
        .additional_properties("indexmap::IndexMap", &TypeRef::new(RustPrimitive::I64).with_vec())
        .build(),
    ],
    SerdeMode::SerializeOnly,
  );
  let filter = schema_struct(
    "Filter",
    vec![
      field("city", TypeRef::new(RustPrimitive::String)),
      field("zip", TypeRef::new(RustPrimitive::U32).with_option()),
    ],
    SerdeMode::SerializeOnly,
  );
  let mut types = vec![
    form,
    filter,
    schema_struct("Address", vec![], SerdeMode::SerializeOnly),
    value_enum("Quality", None),
    value_enum("Level", Some(RustPrimitive::I64)),
    RustType::Enum(EnumDef {
      name: EnumToken::new("Labels"),
      ..Default::default()
    }),
    RustType::TypeAlias(TypeAliasDef {
      name: TypeAliasToken::new("Nickname"),
      docs: Documentation::default(),
      target: TypeRef::new(RustPrimitive::String),
    }),
  ];
  let properties = IndexMap::from([
    (
      "avatar".to_string(),
      MultipartProperty {
        content_type: Some("image/png".to_string()),
        raw_binary: true,
        ..Default::default()
      },
    ),
    (
      "metadata".to_string(),
      MultipartProperty {
        content_type: Some("application/vnd.example+json".to_string()),
        ..Default::default()
      },
    ),
    ("ids".to_string(), styled(PartStyle::PipeDelimited, false)),
    ("filter".to_string(), styled(PartStyle::DeepObject, false)),
    ("options".to_string(), styled(PartStyle::Form, true)),
    (
      "note".to_string(),
      MultipartProperty {
        content_type: Some("application/json".to_string()),
        ..Default::default()
      },
    ),
  ]);
  let mut operations = vec![
    operation("UploadForm", ContentCategory::Multipart, properties),
    operation("Nickname", ContentCategory::Multipart, IndexMap::new()),
  ];

  let warnings = MultipartProcessor::process(&mut types, &mut operations);
  assert!(
    warnings.is_empty(),
    "a multipart-only struct is retyped silently: {warnings:?}"
  );

  let value = |value| MultipartPartKind::Value {
    value,
    content_type: None,
  };
  let styled_kind = |value, style, explode, members| MultipartPartKind::Styled {
    value,
    rfc6570: PartRfc6570 { style, explode },
    members,
  };
  let expected = [
    ("name", value(PartValue::String), None),
    ("fileName", value(PartValue::String), None),
    (
      "avatar",
      MultipartPartKind::File {
        content_type: "image/png".to_string(),
      },
      None,
    ),
    (
      "metadata",
      MultipartPartKind::Value {
        value: PartValue::Json,
        content_type: Some("application/vnd.example+json".to_string()),
      },
      None,
    ),
    (
      "files",
      MultipartPartKind::File {
        content_type: "application/octet-stream".to_string(),
      },
      None,
    ),
    (
      "encoded",
      value(PartValue::SerdeString),
      Some("serde_with::base64::Base64"),
    ),
    ("quality", value(PartValue::StringEnum), None),
    ("level", value(PartValue::Scalar), None),
    ("address", value(PartValue::Json), None),
    ("labels", value(PartValue::Dynamic), None),
    ("alias", value(PartValue::String), None),
    ("created_at", value(PartValue::SerdeString), None),
    ("elapsed", value(PartValue::Dynamic), None),
    ("updated_at", value(PartValue::Dynamic), Some("crate::Stamp")),
    ("count", value(PartValue::Scalar), None),
    (
      "ids",
      styled_kind(PartValue::Scalar, PartStyle::PipeDelimited, false, None),
      None,
    ),
    (
      "filter",
      styled_kind(
        PartValue::Json,
        PartStyle::DeepObject,
        false,
        Some(PartMembers::Named(vec![
          (
            "city".to_string(),
            part_type(TypeRef::new(RustPrimitive::String), PartValue::String),
          ),
          (
            "zip".to_string(),
            part_type(TypeRef::new(RustPrimitive::U32), PartValue::Scalar),
          ),
        ])),
      ),
      None,
    ),
    (
      "options",
      styled_kind(
        PartValue::Dynamic,
        PartStyle::Form,
        true,
        Some(PartMembers::Map(part_type(
          TypeRef::new(RustPrimitive::I64),
          PartValue::Scalar,
        ))),
      ),
      None,
    ),
    (
      "note",
      MultipartPartKind::Value {
        value: PartValue::Json,
        content_type: Some("application/json".to_string()),
      },
      None,
    ),
    (
      "additional_properties",
      MultipartPartKind::Flattened {
        entry: part_type(TypeRef::new(RustPrimitive::I64), PartValue::Scalar),
        repeated: true,
      },
      None,
    ),
  ];

  let fields = operations[0]
    .body
    .as_ref()
    .and_then(|body| body.multipart_fields.as_ref())
    .expect("struct body resolves its fields");
  let actual = fields
    .iter()
    .map(|field| {
      (
        field.part_name.as_str(),
        field.kind.clone(),
        field.serialize_as.as_deref(),
      )
    })
    .collect::<Vec<_>>();
  assert_eq!(actual, expected, "part kinds by field");

  let RustType::Struct(form) = &types[0] else {
    panic!("body struct stays first");
  };
  let retyped = form
    .fields
    .iter()
    .filter(|field| field.rust_type.base_type == RustPrimitive::Bytes)
    .map(|field| field.name.as_str())
    .collect::<Vec<_>>();
  assert_eq!(
    retyped,
    ["avatar", "files", "encoded"],
    "only the raw binary `Value` field is retyped to bytes"
  );

  assert!(
    operations[1]
      .body
      .as_ref()
      .is_some_and(|body| body.multipart_fields.is_none()),
    "a body without a struct keeps no field plan"
  );
}

#[test]
fn test_shared_struct_keeps_raw_binary_fields_untyped() {
  let properties = || {
    IndexMap::from([(
      "blob".to_string(),
      MultipartProperty {
        raw_binary: true,
        ..Default::default()
      },
    )])
  };
  let shared = |serde_mode| {
    schema_struct(
      "Shared",
      vec![field("blob", TypeRef::new(RustPrimitive::Value))],
      serde_mode,
    )
  };
  let multipart = || operation("Shared", ContentCategory::Multipart, properties());

  let cases = [
    ("a JSON response", vec![shared(SerdeMode::Both)], vec![multipart()]),
    (
      "another type's field",
      vec![
        shared(SerdeMode::SerializeOnly),
        schema_struct(
          "Holder",
          vec![field("shared", TypeRef::new("Shared"))],
          SerdeMode::SerializeOnly,
        ),
      ],
      vec![multipart()],
    ),
    (
      "a JSON request body",
      vec![shared(SerdeMode::SerializeOnly)],
      vec![multipart(), operation("Shared", ContentCategory::Json, IndexMap::new())],
    ),
  ];
  for (usage, mut types, mut operations) in cases {
    let warnings = MultipartProcessor::process(&mut types, &mut operations);

    let RustType::Struct(def) = &types[0] else {
      panic!("{usage}: body struct missing");
    };
    assert_eq!(
      def.fields[0].rust_type.base_type,
      RustPrimitive::Value,
      "{usage}: shared struct keeps `serde_json::Value`"
    );
    assert_eq!(warnings.len(), 1, "{usage}: one warning per untyped raw binary field");
    assert!(
      warnings[0].to_string().contains("`blob` is raw binary"),
      "{usage}: unexpected warning {}",
      warnings[0]
    );
  }
}
