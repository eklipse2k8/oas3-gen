use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::generator::{
  ast::{RustPrimitive, TypeRef, ValidationAttribute},
  converter::{SchemaExt, fields::FieldConverter},
};

fn create_string_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
    ..Default::default()
  }
}

fn create_number_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Number)),
    ..Default::default()
  }
}

fn create_integer_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Integer)),
    ..Default::default()
  }
}

fn create_array_schema() -> ObjectSchema {
  ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
    ..Default::default()
  }
}

#[test]
fn test_is_single_type() {
  let string_schema = create_string_schema();
  assert!(string_schema.is_single_type(SchemaType::String));
  assert!(!string_schema.is_single_type(SchemaType::Number));

  let number_schema = create_number_schema();
  assert!(number_schema.is_single_type(SchemaType::Number));
  assert!(!number_schema.is_single_type(SchemaType::String));
}

#[test]
fn test_extract_docs() {
  let schema = ObjectSchema::default();
  let docs = FieldConverter::extract_docs(&schema);
  assert!(docs.is_empty());

  let schema = ObjectSchema {
    description: Some("Test description\nSecond line".to_string()),
    ..Default::default()
  };
  let docs = FieldConverter::extract_docs(&schema);
  assert_eq!(docs.lines().len(), 2);
  assert_eq!(docs.lines()[0], "Test description");
  assert_eq!(docs.lines()[1], "Second line");
}

#[test]
fn test_extract_default_value() {
  let schema = ObjectSchema {
    default: Some(json!("default_value")),
    ..Default::default()
  };
  assert_eq!(
    FieldConverter::extract_default_value(&schema),
    Some(json!("default_value"))
  );

  let schema = ObjectSchema {
    const_value: Some(json!("const_value")),
    ..Default::default()
  };
  assert_eq!(
    FieldConverter::extract_default_value(&schema),
    Some(json!("const_value"))
  );

  let schema = ObjectSchema {
    enum_values: vec![json!("only_value")],
    ..Default::default()
  };
  assert_eq!(
    FieldConverter::extract_default_value(&schema),
    Some(json!("only_value"))
  );

  let schema = ObjectSchema {
    default: Some(json!("default")),
    const_value: Some(json!("const")),
    enum_values: vec![json!("enum")],
    ..Default::default()
  };
  assert_eq!(FieldConverter::extract_default_value(&schema), Some(json!("default")));

  let schema = ObjectSchema::default();
  assert_eq!(FieldConverter::extract_default_value(&schema), None);
}

#[test]
fn test_extract_all_validation_combined() {
  let mut schema = create_string_schema();
  schema.description = Some("Test field".to_string());
  schema.format = Some("email".to_string());

  let type_ref = TypeRef::new(RustPrimitive::String);

  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  let docs = FieldConverter::extract_docs(&schema);
  let default_value = FieldConverter::extract_default_value(&schema);

  assert!(!docs.is_empty());
  assert!(attrs.contains(&ValidationAttribute::Email));
  assert!(default_value.is_none());
}

#[test]
fn test_extract_parameter_metadata() {
  let mut schema = create_string_schema();
  schema.min_length = Some(5);
  schema.max_length = Some(50);
  schema.default = Some(json!("default_value"));

  let type_ref = TypeRef::new(RustPrimitive::String);

  let (validation_attrs, default_value) = FieldConverter::extract_parameter_metadata("param", true, &schema, &type_ref);

  assert_eq!(
    validation_attrs,
    vec![ValidationAttribute::Length {
      min: Some(5),
      max: Some(50)
    }]
  );
  assert_eq!(default_value, Some(json!("default_value")));
}

#[test]
fn test_extract_all_validation_email_format() {
  let mut schema = create_string_schema();
  schema.format = Some("email".to_string());
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.contains(&ValidationAttribute::Email));
}

#[test]
fn test_extract_all_validation_url_format() {
  let mut schema = create_string_schema();
  schema.format = Some("url".to_string());
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.contains(&ValidationAttribute::Url));

  schema.format = Some("uri".to_string());
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.contains(&ValidationAttribute::Url));
}

#[test]
fn test_extract_all_validation_integer_range() {
  let mut schema = create_integer_schema();
  schema.minimum = Some(json!(0).as_number().unwrap().clone());
  schema.maximum = Some(json!(100).as_number().unwrap().clone());
  let type_ref = TypeRef::new(RustPrimitive::I32);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(attrs.len(), 1);
  assert_eq!(
    attrs[0],
    ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: Some(json!(0).as_number().unwrap().clone()),
      max: Some(json!(100).as_number().unwrap().clone()),
      exclusive_min: None,
      exclusive_max: None,
    }
  );
}

#[test]
fn test_extract_all_validation_exclusive_range() {
  let mut schema = create_number_schema();
  schema.exclusive_minimum = Some(json!(0).as_number().unwrap().clone());
  schema.exclusive_maximum = Some(json!(100).as_number().unwrap().clone());
  let type_ref = TypeRef::new(RustPrimitive::I32);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(attrs.len(), 1);
  assert_eq!(
    attrs[0],
    ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: None,
      max: None,
      exclusive_min: Some(json!(0).as_number().unwrap().clone()),
      exclusive_max: Some(json!(100).as_number().unwrap().clone()),
    }
  );
}

#[test]
fn test_extract_all_validation_no_range_when_empty() {
  let schema = create_number_schema();
  let type_ref = TypeRef::new(RustPrimitive::I32);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.is_empty());
}

#[test]
fn test_extract_all_validation_string_length() {
  let mut schema = create_string_schema();
  schema.min_length = Some(1);
  schema.max_length = Some(100);
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(1),
      max: Some(100)
    }]
  );
}

#[test]
fn test_extract_all_validation_string_length_min_only() {
  let mut schema = create_string_schema();
  schema.min_length = Some(5);
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(5),
      max: None
    }]
  );
}

#[test]
fn test_extract_all_validation_string_length_max_only() {
  let mut schema = create_string_schema();
  schema.max_length = Some(20);
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: None,
      max: Some(20)
    }]
  );
}

#[test]
fn test_extract_all_validation_string_length_skipped_for_non_string_format() {
  let mut schema = create_string_schema();
  schema.min_length = Some(1);
  schema.format = Some("date".to_string());
  let type_ref = TypeRef::new(RustPrimitive::Date);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.is_empty());
}

#[test]
fn test_extract_all_validation_required_implies_min_length() {
  let schema = create_string_schema();
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", true, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(1),
      max: None
    }]
  );
}

#[test]
fn test_extract_all_validation_regex() {
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.contains(&ValidationAttribute::Regex("^[a-z]+$".to_string())));
}

#[test]
fn test_extract_all_validation_regex_skipped_for_enums() {
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());
  schema.enum_values = vec![json!("value1"), json!("value2")];
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(attrs.is_empty());
}

#[test]
fn test_extract_all_validation_regex_skipped_for_datetime() {
  let mut schema = create_string_schema();
  schema.pattern = Some("^[0-9]+$".to_string());
  let type_ref = TypeRef::new(RustPrimitive::DateTime);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(!attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))));
}

#[test]
fn test_extract_all_validation_regex_skipped_for_date() {
  let mut schema = create_string_schema();
  schema.pattern = Some("pattern".to_string());
  let type_ref = TypeRef::new(RustPrimitive::Date);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(!attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))));
}

#[test]
fn test_extract_all_validation_regex_skipped_for_uuid() {
  let mut schema = create_string_schema();
  schema.pattern = Some("pattern".to_string());
  let type_ref = TypeRef::new(RustPrimitive::Uuid);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(!attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))));
}

#[test]
fn test_extract_all_validation_invalid_regex_skipped() {
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z+$".to_string());
  let type_ref = TypeRef::new(RustPrimitive::String);
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert!(!attrs.iter().any(|a| matches!(a, ValidationAttribute::Regex(_))));
}

#[test]
fn test_extract_all_validation_array_length() {
  let mut schema = create_array_schema();
  schema.min_items = Some(1);
  schema.max_items = Some(10);
  let type_ref = TypeRef::new(RustPrimitive::String).with_vec();
  let attrs = FieldConverter::extract_all_validation("test", false, &schema, &type_ref);
  assert_eq!(
    attrs,
    vec![ValidationAttribute::Length {
      min: Some(1),
      max: Some(10)
    }]
  );
}
