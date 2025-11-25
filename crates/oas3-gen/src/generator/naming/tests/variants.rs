use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::generator::{
  ast::{TypeRef, VariantContent, VariantDef},
  naming::variants::{VariantNameNormalizer, infer_variant_name, strip_common_affixes},
};

#[test]
fn test_normalize_string_basic() {
  let val = json!("active");
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Active");
  assert_eq!(res.rename_value, "active");
}

#[test]
fn test_normalize_string_snake_case() {
  let val = json!("pending_approval");
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "PendingApproval");
  assert_eq!(res.rename_value, "pending_approval");
}

#[test]
fn test_normalize_string_kebab_case() {
  let val = json!("pending-approval");
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "PendingApproval");
  assert_eq!(res.rename_value, "pending-approval");
}

#[test]
fn test_normalize_positive_int() {
  let val = json!(404);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value404");
  assert_eq!(res.rename_value, "404");
}

#[test]
fn test_normalize_negative_int() {
  let val = json!(-42);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value-42");
  assert_eq!(res.rename_value, "-42");
}

#[test]
fn test_normalize_zero() {
  let val = json!(0);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value0");
  assert_eq!(res.rename_value, "0");
}

#[test]
#[allow(clippy::approx_constant)]
fn test_normalize_float_positive() {
  let val = json!(3.14);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value3_14");
  assert_eq!(res.rename_value, "3.14");
}

#[test]
fn test_normalize_float_negative() {
  let val = json!(-2.5);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "Value_2_5");
  assert_eq!(res.rename_value, "-2.5");
}

#[test]
fn test_normalize_bool_true() {
  let val = json!(true);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "True");
  assert_eq!(res.rename_value, "true");
}

#[test]
fn test_normalize_bool_false() {
  let val = json!(false);
  let res = VariantNameNormalizer::normalize(&val).unwrap();
  assert_eq!(res.name, "False");
  assert_eq!(res.rename_value, "false");
}

#[test]
fn test_normalize_null_returns_none() {
  let val = json!(null);
  assert!(VariantNameNormalizer::normalize(&val).is_none());
}

#[test]
fn test_normalize_object_returns_none() {
  let val = json!({"key": "value"});
  assert!(VariantNameNormalizer::normalize(&val).is_none());
}

#[test]
fn test_normalize_array_returns_none() {
  let val = json!([1, 2, 3]);
  assert!(VariantNameNormalizer::normalize(&val).is_none());
}

#[test]
fn test_infer_variant_name_enum() {
  let schema = ObjectSchema {
    enum_values: vec![json!("a"), json!("b")],
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Enum");
}

#[test]
fn test_infer_variant_name_string() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "String");
}

#[test]
fn test_infer_variant_name_number() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Number");
}

#[test]
fn test_infer_variant_name_integer() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Integer");
}

#[test]
fn test_infer_variant_name_boolean() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Boolean)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Boolean");
}

#[test]
fn test_infer_variant_name_array() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Array");
}

#[test]
fn test_infer_variant_name_object() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Object");
}

#[test]
fn test_infer_variant_name_null() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Null)),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Null");
}

#[test]
fn test_infer_variant_name_multiple_types() {
  let schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Multiple(vec![SchemaType::String, SchemaType::Number])),
    ..Default::default()
  };
  assert_eq!(infer_variant_name(&schema, 0), "Mixed");
}

#[test]
fn test_infer_variant_name_no_type() {
  let schema = ObjectSchema::default();
  assert_eq!(infer_variant_name(&schema, 5), "Variant5");
}

#[test]
fn test_strip_common_affixes_empty_slice() {
  let mut variants = vec![];
  strip_common_affixes(&mut variants);
  assert!(variants.is_empty());
}

#[test]
fn test_strip_common_affixes_single_variant() {
  let mut variants = vec![VariantDef {
    name: "UserResponse".into(),
    docs: vec![],
    content: VariantContent::Unit,
    serde_attrs: vec![],
    deprecated: false,
  }];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "UserResponse");
}

#[test]
fn test_strip_common_affixes_common_suffix() {
  let mut variants = vec![
    VariantDef {
      name: "CreateResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "UpdateResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "Create");
  assert_eq!(variants[1].name, "Update");
}

#[test]
fn test_strip_common_affixes_common_prefix() {
  let mut variants = vec![
    VariantDef {
      name: "UserCreate".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "UserUpdate".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "Create");
  assert_eq!(variants[1].name, "Update");
}

#[test]
fn test_strip_common_affixes_both_prefix_and_suffix() {
  let mut variants = vec![
    VariantDef {
      name: "UserCreateResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "UserUpdateResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "Create");
  assert_eq!(variants[1].name, "Update");
}

#[test]
fn test_strip_common_affixes_no_common_parts() {
  let mut variants = vec![
    VariantDef {
      name: "CreateUser".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "DeletePost".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "CreateUser");
  assert_eq!(variants[1].name, "DeletePost");
}

#[test]
fn test_strip_common_affixes_collision_prevents_stripping() {
  let mut variants = vec![
    VariantDef {
      name: "UserResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "UserResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "UserResponse");
  assert_eq!(variants[1].name, "UserResponse");
}

#[test]
fn test_strip_common_affixes_would_create_empty_name() {
  let mut variants = vec![
    VariantDef {
      name: "Response".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "Response".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "Response");
  assert_eq!(variants[1].name, "Response");
}

#[test]
fn test_strip_common_affixes_preserves_content() {
  let tuple_type = TypeRef::new("TestStruct");
  let mut variants = vec![
    VariantDef {
      name: "CreateResponse".into(),
      docs: vec![],
      content: VariantContent::Tuple(vec![tuple_type]),
      serde_attrs: vec![],
      deprecated: false,
    },
    VariantDef {
      name: "UpdateResponse".into(),
      docs: vec![],
      content: VariantContent::Unit,
      serde_attrs: vec![],
      deprecated: false,
    },
  ];
  strip_common_affixes(&mut variants);
  assert_eq!(variants[0].name, "Create");
  assert_eq!(variants[1].name, "Update");
  assert!(matches!(variants[0].content, VariantContent::Tuple(_)));
  assert!(matches!(variants[1].content, VariantContent::Unit));
}
