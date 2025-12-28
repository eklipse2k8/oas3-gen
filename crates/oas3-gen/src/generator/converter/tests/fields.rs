use std::collections::{BTreeMap, HashMap};

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{OuterAttr, RustPrimitive, RustType, SerdeAsFieldAttr},
    converter::{CodegenConfig, SchemaConverter},
  },
  tests::common::create_test_graph,
};

fn config_with_customizations(customizations: HashMap<String, String>) -> CodegenConfig {
  CodegenConfig {
    customizations,
    ..Default::default()
  }
}

#[test]
fn test_datetime_field_with_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "created_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["created_at".to_string()];

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Event".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Event", graph.get_schema("Event").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "created_at")
    .expect("created_at field should exist");

  assert!(
    field.serde_as_attr.is_some(),
    "Field should have serde_as_attr when customization is provided"
  );

  let serde_as = field.serde_as_attr.as_ref().unwrap();
  assert_eq!(
    *serde_as,
    SerdeAsFieldAttr::CustomOverride {
      custom_type: "crate::MyDateTime".to_string(),
      optional: false,
      is_array: false,
    }
  );

  assert!(
    struct_def.outer_attrs.contains(&OuterAttr::SerdeAs),
    "Struct should have #[serde_with::serde_as] outer attribute"
  );

  Ok(())
}

#[test]
fn test_optional_datetime_field_with_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "updated_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Event".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Event", graph.get_schema("Event").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "updated_at")
    .expect("updated_at field should exist");

  let serde_as = field.serde_as_attr.as_ref().expect("Should have serde_as_attr");
  assert_eq!(
    *serde_as,
    SerdeAsFieldAttr::CustomOverride {
      custom_type: "crate::MyDateTime".to_string(),
      optional: true,
      is_array: false,
    }
  );

  Ok(())
}

#[test]
fn test_array_of_datetime_with_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "timestamps".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::Array)),
      items: Some(Box::new(Schema::Object(Box::new(ObjectOrReference::Object(
        ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          format: Some("date-time".to_string()),
          ..Default::default()
        },
      ))))),
      ..Default::default()
    }),
  );
  schema.required = vec!["timestamps".to_string()];

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Event".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Event", graph.get_schema("Event").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "timestamps")
    .expect("timestamps field should exist");

  let serde_as = field.serde_as_attr.as_ref().expect("Should have serde_as_attr");
  assert_eq!(
    *serde_as,
    SerdeAsFieldAttr::CustomOverride {
      custom_type: "crate::MyDateTime".to_string(),
      optional: false,
      is_array: true,
    }
  );

  Ok(())
}

#[test]
fn test_date_field_with_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "birth_date".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["birth_date".to_string()];

  let customizations = HashMap::from([("date".to_string(), "crate::MyDate".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Person".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Person", graph.get_schema("Person").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "birth_date")
    .expect("birth_date field should exist");

  let serde_as = field.serde_as_attr.as_ref().expect("Should have serde_as_attr");
  assert_eq!(
    *serde_as,
    SerdeAsFieldAttr::CustomOverride {
      custom_type: "crate::MyDate".to_string(),
      optional: false,
      is_array: false,
    }
  );

  Ok(())
}

#[test]
fn test_uuid_field_with_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("uuid".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["id".to_string()];

  let customizations = HashMap::from([("uuid".to_string(), "crate::MyUuid".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "id")
    .expect("id field should exist");

  let serde_as = field.serde_as_attr.as_ref().expect("Should have serde_as_attr");
  assert_eq!(
    *serde_as,
    SerdeAsFieldAttr::CustomOverride {
      custom_type: "crate::MyUuid".to_string(),
      optional: false,
      is_array: false,
    }
  );

  Ok(())
}

#[test]
fn test_no_serde_as_attr_without_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "created_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["created_at".to_string()];

  let graph = create_test_graph(BTreeMap::from([("Event".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &CodegenConfig::default());
  let result = converter.convert_schema("Event", graph.get_schema("Event").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "created_at")
    .expect("created_at field should exist");

  assert!(
    field.serde_as_attr.is_none(),
    "Field should not have serde_as_attr when no customization is provided"
  );

  assert!(
    !struct_def.outer_attrs.contains(&OuterAttr::SerdeAs),
    "Struct should not have #[serde_with::serde_as] when no fields have serde_as_attr"
  );

  Ok(())
}

#[test]
fn test_string_field_no_customization() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "name".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      ..Default::default()
    }),
  );
  schema.required = vec!["name".to_string()];

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Person".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Person", graph.get_schema("Person").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "name")
    .expect("name field should exist");

  assert_eq!(field.rust_type.base_type, RustPrimitive::String);
  assert!(
    field.serde_as_attr.is_none(),
    "String fields should not be customized by date_time customization"
  );

  Ok(())
}

#[test]
fn test_multiple_customizations() -> anyhow::Result<()> {
  let mut schema = ObjectSchema {
    schema_type: Some(SchemaTypeSet::Single(SchemaType::Object)),
    ..Default::default()
  };
  schema.properties.insert(
    "created_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.properties.insert(
    "birth_date".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date".to_string()),
      ..Default::default()
    }),
  );
  schema.properties.insert(
    "id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("uuid".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["created_at".to_string(), "birth_date".to_string(), "id".to_string()];

  let customizations = HashMap::from([
    ("date_time".to_string(), "crate::MyDateTime".to_string()),
    ("date".to_string(), "crate::MyDate".to_string()),
    ("uuid".to_string(), "crate::MyUuid".to_string()),
  ]);

  let graph = create_test_graph(BTreeMap::from([("Entity".to_string(), schema)]));
  let converter = SchemaConverter::new(&graph, &config_with_customizations(customizations));
  let result = converter.convert_schema("Entity", graph.get_schema("Entity").unwrap(), None)?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let created_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "created_at")
    .expect("created_at field should exist");
  assert!(matches!(
    created_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyDateTime"
  ));

  let birth_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "birth_date")
    .expect("birth_date field should exist");
  assert!(matches!(
    birth_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyDate"
  ));

  let id_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "id")
    .expect("id field should exist");
  assert!(matches!(
    id_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyUuid"
  ));

  Ok(())
}
