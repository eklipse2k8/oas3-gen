use std::collections::{BTreeMap, HashMap};

use oas3::spec::{ObjectOrReference, ObjectSchema, Schema, SchemaType, SchemaTypeSet};

use crate::{
  generator::{
    ast::{OuterAttr, RustPrimitive, RustType, SerdeAsFieldAttr},
    converter::{CodegenConfig, SchemaConverter, fields::FieldConverter},
  },
  tests::common::{create_test_context, create_test_graph, make_field},
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
    "sploot_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["sploot_at".to_string()];

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Frappe".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Frappe", graph.get("Frappe").unwrap())?;

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
    .find(|f| f.name == "sploot_at")
    .expect("sploot_at field should exist");

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
    "waddle_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Frappe".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Frappe", graph.get("Frappe").unwrap())?;

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
    .find(|f| f.name == "waddle_at")
    .expect("waddle_at field should exist");

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
    "toebeans".to_string(),
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
  schema.required = vec!["toebeans".to_string()];

  let customizations = HashMap::from([("date_time".to_string(), "crate::MyDateTime".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Frappe".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Frappe", graph.get("Frappe").unwrap())?;

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
    .find(|f| f.name == "toebeans")
    .expect("toebeans field should exist");

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
    "corgi_date".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["corgi_date".to_string()];

  let customizations = HashMap::from([("date".to_string(), "crate::MyDate".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Corgi".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgi", graph.get("Corgi").unwrap())?;

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
    .find(|f| f.name == "corgi_date")
    .expect("corgi_date field should exist");

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
    "tag_id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("uuid".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["tag_id".to_string()];

  let customizations = HashMap::from([("uuid".to_string(), "crate::MyUuid".to_string())]);
  let graph = create_test_graph(BTreeMap::from([("Cardigan".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Cardigan", graph.get("Cardigan").unwrap())?;

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
    .find(|f| f.name == "tag_id")
    .expect("tag_id field should exist");

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
    "sploot_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["sploot_at".to_string()];

  let graph = create_test_graph(BTreeMap::from([("Frappe".to_string(), schema)]));
  let context = create_test_context(graph.clone(), CodegenConfig::default());
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Frappe", graph.get("Frappe").unwrap())?;

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
    .find(|f| f.name == "sploot_at")
    .expect("sploot_at field should exist");

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
fn field_name_deduplication() {
  struct Case {
    name: &'static str,
    fields: Vec<(&'static str, bool)>,
    expected_names: Vec<&'static str>,
  }

  let cases = [
    Case {
      name: "no duplicates",
      fields: vec![("loaf", false), ("sploot", false), ("floof", false)],
      expected_names: vec!["loaf", "sploot", "floof"],
    },
    Case {
      name: "empty input",
      fields: vec![],
      expected_names: vec![],
    },
    Case {
      name: "all non-deprecated renamed with suffix",
      fields: vec![("loaf", false), ("loaf", false), ("loaf", false)],
      expected_names: vec!["loaf", "loaf_2", "loaf_3"],
    },
    Case {
      name: "deprecated removed when mixed with non-deprecated",
      fields: vec![("loaf", true), ("loaf", false), ("sploot", false)],
      expected_names: vec!["loaf", "sploot"],
    },
    Case {
      name: "all deprecated renamed with suffix",
      fields: vec![("loaf", true), ("loaf", true)],
      expected_names: vec!["loaf", "loaf_2"],
    },
    Case {
      name: "multiple groups",
      fields: vec![("loaf", false), ("sploot", true), ("loaf", false), ("sploot", false)],
      expected_names: vec!["loaf", "loaf_2", "sploot"],
    },
  ];

  for case in cases {
    let fields = case.fields.iter().map(|(n, d)| make_field(n, *d)).collect::<Vec<_>>();
    let result = FieldConverter::deduplicate_names(fields);
    let names = result.iter().map(|f| f.name.as_str()).collect::<Vec<_>>();

    assert_eq!(names.len(), case.expected_names.len(), "{}: length mismatch", case.name);

    for expected in &case.expected_names {
      assert!(names.contains(expected), "{}: missing '{}'", case.name, expected);
    }
  }
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
  let graph = create_test_graph(BTreeMap::from([("Corgi".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Corgi", graph.get("Corgi").unwrap())?;

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
    "sploot_at".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date-time".to_string()),
      ..Default::default()
    }),
  );
  schema.properties.insert(
    "corgi_date".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("date".to_string()),
      ..Default::default()
    }),
  );
  schema.properties.insert(
    "tag_id".to_string(),
    ObjectOrReference::Object(ObjectSchema {
      schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
      format: Some("uuid".to_string()),
      ..Default::default()
    }),
  );
  schema.required = vec!["sploot_at".to_string(), "corgi_date".to_string(), "tag_id".to_string()];

  let customizations = HashMap::from([
    ("date_time".to_string(), "crate::MyDateTime".to_string()),
    ("date".to_string(), "crate::MyDate".to_string()),
    ("uuid".to_string(), "crate::MyUuid".to_string()),
  ]);

  let graph = create_test_graph(BTreeMap::from([("Cardigan".to_string(), schema)]));
  let context = create_test_context(graph.clone(), config_with_customizations(customizations));
  let converter = SchemaConverter::new(&context);
  let result = converter.convert_schema("Cardigan", graph.get("Cardigan").unwrap())?;

  let struct_def = result
    .iter()
    .find_map(|ty| match ty {
      RustType::Struct(def) => Some(def),
      _ => None,
    })
    .expect("Struct should be present");

  let sploot_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "sploot_at")
    .expect("sploot_at field should exist");
  assert!(matches!(
    sploot_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyDateTime"
  ));

  let birth_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "corgi_date")
    .expect("corgi_date field should exist");
  assert!(matches!(
    birth_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyDate"
  ));

  let tag_id_field = struct_def
    .fields
    .iter()
    .find(|f| f.name == "tag_id")
    .expect("tag_id field should exist");
  assert!(matches!(
    tag_id_field.serde_as_attr,
    Some(SerdeAsFieldAttr::CustomOverride { ref custom_type, .. }) if custom_type == "crate::MyUuid"
  ));

  Ok(())
}
