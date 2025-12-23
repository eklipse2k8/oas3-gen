use oas3::spec::{ObjectSchema, SchemaType, SchemaTypeSet};
use serde_json::json;

use crate::generator::{
  ast::{RustPrimitive, TypeRef, ValidationAttribute},
  converter::{SchemaExt, metadata::*},
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

fn make_extractor<'a>(
  prop_name: &'a str,
  is_required: bool,
  schema: &'a ObjectSchema,
  type_ref: &'a TypeRef,
) -> MetadataExtractor<'a> {
  MetadataExtractor::new(prop_name, is_required, schema, type_ref)
}

#[test]
fn test_is_non_string_format() {
  let non_string_formats = vec!["date", "date-time", "duration", "time", "binary", "byte", "uuid"];
  for format in non_string_formats {
    assert!(
      MetadataExtractor::is_non_string_format(format),
      "Format {format} should be non-string"
    );
  }

  let string_formats = vec!["email", "uri", "url", "other", "ipv4", "ipv6"];
  for format in string_formats {
    assert!(
      !MetadataExtractor::is_non_string_format(format),
      "Format {format} should be string"
    );
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
  let type_ref = TypeRef::default();
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert!(extractor.extract_docs().is_empty());

  let schema = ObjectSchema {
    description: Some("Test description\nSecond line".to_string()),
    ..Default::default()
  };
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let docs = extractor.extract_docs();
  assert_eq!(docs.lines().len(), 2);
  assert_eq!(docs.lines()[0], "Test description");
  assert_eq!(docs.lines()[1], "Second line");
}

#[test]
fn test_extract_default_value() {
  let type_ref = TypeRef::default();

  // Test default
  let schema = ObjectSchema {
    default: Some(json!("default_value")),
    ..Default::default()
  };
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.extract_default_value(), Some(json!("default_value")));

  // Test const
  let schema = ObjectSchema {
    const_value: Some(json!("const_value")),
    ..Default::default()
  };
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.extract_default_value(), Some(json!("const_value")));

  // Test single enum
  let schema = ObjectSchema {
    enum_values: vec![json!("only_value")],
    ..Default::default()
  };
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.extract_default_value(), Some(json!("only_value")));

  // Test priority: default > const > enum
  let schema = ObjectSchema {
    default: Some(json!("default")),
    const_value: Some(json!("const")),
    enum_values: vec![json!("enum")],
    ..Default::default()
  };
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.extract_default_value(), Some(json!("default")));

  // Test None
  let schema = ObjectSchema::default();
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.extract_default_value(), None);
}

#[test]
fn test_extract_validation_pattern() {
  let type_ref = TypeRef::new(RustPrimitive::String);

  // Valid pattern
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());
  let extractor = make_extractor("test_field", false, &schema, &type_ref);
  assert_eq!(extractor.extract_validation_pattern(), Some(&"^[a-z]+$".to_string()));

  // Non-string type
  let mut schema = create_number_schema();
  schema.pattern = Some("^[0-9]+$".to_string());
  let extractor = make_extractor("test_field", false, &schema, &type_ref);
  assert_eq!(extractor.extract_validation_pattern(), None);

  // Non-string format
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());
  schema.format = Some("date".to_string());
  let extractor = make_extractor("test_field", false, &schema, &type_ref);
  assert_eq!(extractor.extract_validation_pattern(), None);

  // With enum
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z]+$".to_string());
  schema.enum_values = vec![json!("value1"), json!("value2")];
  let extractor = make_extractor("test_field", false, &schema, &type_ref);
  assert_eq!(extractor.extract_validation_pattern(), None);

  // Invalid regex
  let mut schema = create_string_schema();
  schema.pattern = Some("^[a-z+$".to_string());
  let extractor = make_extractor("test_field", false, &schema, &type_ref);
  assert_eq!(extractor.extract_validation_pattern(), None);
}

#[test]
fn test_filter_regex_validation() {
  let schema = ObjectSchema::default();

  // DateTime
  let type_ref = TypeRef::new(RustPrimitive::DateTime);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.filter_regex_validation(Some("pattern".to_string())), None);

  // Date
  let type_ref = TypeRef::new(RustPrimitive::Date);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.filter_regex_validation(Some("pattern".to_string())), None);

  // Uuid
  let type_ref = TypeRef::new(RustPrimitive::Uuid);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(extractor.filter_regex_validation(Some("pattern".to_string())), None);

  // String (should keep regex)
  let type_ref = TypeRef::new(RustPrimitive::String);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  assert_eq!(
    extractor.filter_regex_validation(Some("pattern".to_string())),
    Some("pattern".to_string())
  );
}

#[test]
fn test_extract_validation_attrs() {
  // Email
  let mut schema = create_string_schema();
  schema.format = Some("email".to_string());
  let type_ref = TypeRef::new(RustPrimitive::String);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let attrs = extractor.extract_validation_attrs();
  assert_eq!(attrs, vec![ValidationAttribute::Email]);

  // URL
  let mut schema = create_string_schema();
  schema.format = Some("url".to_string());
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let attrs = extractor.extract_validation_attrs();
  assert_eq!(attrs, vec![ValidationAttribute::Url]);

  // URI (should map to URL)
  let mut schema = create_string_schema();
  schema.format = Some("uri".to_string());
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let attrs = extractor.extract_validation_attrs();
  assert_eq!(attrs, vec![ValidationAttribute::Url]);

  // Integer Range
  let mut schema = create_integer_schema();
  schema.minimum = Some(json!(0).as_number().unwrap().clone());
  schema.maximum = Some(json!(100).as_number().unwrap().clone());
  let type_ref = TypeRef::new(RustPrimitive::I32);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let attrs = extractor.extract_validation_attrs();
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
fn test_build_range_validation_attr() {
  // Min/Max
  let mut schema = create_number_schema();
  schema.minimum = Some(json!(0).as_number().unwrap().clone());
  schema.maximum = Some(json!(100).as_number().unwrap().clone());
  let type_ref = TypeRef::new(RustPrimitive::I32);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_range_validation_attr();
  assert_eq!(
    result,
    Some(ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: Some(json!(0).as_number().unwrap().clone()),
      max: Some(json!(100).as_number().unwrap().clone()),
      exclusive_min: None,
      exclusive_max: None,
    })
  );

  // Exclusive Min/Max
  let mut schema = create_number_schema();
  schema.exclusive_minimum = Some(json!(0).as_number().unwrap().clone());
  schema.exclusive_maximum = Some(json!(100).as_number().unwrap().clone());
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_range_validation_attr();
  assert_eq!(
    result,
    Some(ValidationAttribute::Range {
      primitive: RustPrimitive::I32,
      min: None,
      max: None,
      exclusive_min: Some(json!(0).as_number().unwrap().clone()),
      exclusive_max: Some(json!(100).as_number().unwrap().clone()),
    })
  );

  // None
  let schema = create_number_schema();
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_range_validation_attr();
  assert_eq!(result, None);
}

#[test]
fn test_build_string_length_validation_attr() {
  let type_ref = TypeRef::new(RustPrimitive::String);

  // Min/Max
  let mut schema = create_string_schema();
  schema.min_length = Some(1);
  schema.max_length = Some(100);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_string_length_validation_attr();
  assert_eq!(
    result,
    Some(ValidationAttribute::Length {
      min: Some(1),
      max: Some(100)
    })
  );

  // Non-string format
  let mut schema = create_string_schema();
  schema.min_length = Some(1);
  schema.format = Some("date".to_string());
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_string_length_validation_attr();
  assert_eq!(result, None);

  // Required (implies min_length=1 if not set)
  let schema = create_string_schema();
  let extractor = make_extractor("test", true, &schema, &type_ref);
  let result = extractor.build_string_length_validation_attr();
  assert_eq!(
    result,
    Some(ValidationAttribute::Length {
      min: Some(1),
      max: None
    })
  );
}

#[test]
fn test_build_array_length_validation_attr() {
  let type_ref = TypeRef::new(RustPrimitive::String).with_vec();

  let mut schema = create_array_schema();
  schema.min_items = Some(1);
  schema.max_items = Some(10);
  let extractor = make_extractor("test", false, &schema, &type_ref);
  let result = extractor.build_array_length_validation_attr();
  assert_eq!(
    result,
    Some(ValidationAttribute::Length {
      min: Some(1),
      max: Some(10)
    })
  );
}

#[test]
fn test_build_length_attribute() {
  // Both
  assert_eq!(
    MetadataExtractor::build_length_attribute(Some(1), Some(10), false),
    Some(ValidationAttribute::Length {
      min: Some(1),
      max: Some(10)
    })
  );

  // Min only
  assert_eq!(
    MetadataExtractor::build_length_attribute(Some(5), None, false),
    Some(ValidationAttribute::Length {
      min: Some(5),
      max: None
    })
  );

  // Max only
  assert_eq!(
    MetadataExtractor::build_length_attribute(None, Some(20), false),
    Some(ValidationAttribute::Length {
      min: None,
      max: Some(20)
    })
  );

  // Required non-empty
  assert_eq!(
    MetadataExtractor::build_length_attribute(None, None, true),
    Some(ValidationAttribute::Length {
      min: Some(1),
      max: None
    })
  );

  // None
  assert_eq!(MetadataExtractor::build_length_attribute(None, None, false), None);
}

#[test]
fn test_field_metadata_from_schema() {
  let mut schema = create_string_schema();
  schema.description = Some("Test field".to_string());
  schema.deprecated = Some(true);
  schema.format = Some("email".to_string());
  schema.multiple_of = Some(json!(2).as_number().unwrap().clone());

  let type_ref = TypeRef::new(RustPrimitive::String);

  let metadata = FieldMetadata::from_schema("test", false, &schema, &type_ref);

  assert!(!metadata.docs.is_empty());
  assert!(metadata.deprecated);
  assert!(!metadata.validation_attrs.is_empty());
  assert!(metadata.multiple_of.is_some());
}
