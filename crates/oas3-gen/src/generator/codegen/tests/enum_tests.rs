use crate::generator::{
  ast::{EnumDef, VariantContent, VariantDef},
  codegen::{Visibility, enums::generate_enum},
};

#[test]
fn test_case_insensitive_enum_generation() {
  let def = EnumDef {
    name: "Status".to_string(),
    docs: vec![],
    variants: vec![
      VariantDef {
        name: "Active".to_string(),
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![r#"rename = "active""#.to_string()],
        deprecated: false,
      },
      VariantDef {
        name: "InProgress".to_string(),
        docs: vec![],
        content: VariantContent::Unit,
        serde_attrs: vec![r#"rename = "in-progress""#.to_string()],
        deprecated: false,
      },
    ],
    discriminator: None,
    derives: vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "Serialize".to_string(),
      "Deserialize".to_string(),
    ],
    serde_attrs: vec![],
    outer_attrs: vec![],
    case_insensitive: true,
  };

  let tokens = generate_enum(&def, Visibility::Public);
  let code = tokens.to_string();

  let parts: Vec<&str> = code.split("enum Status").collect();
  assert_eq!(parts.len(), 2);
  let derive_part = parts[0];
  let impl_part = parts[1];

  assert!(
    !derive_part.contains("Deserialize"),
    "Deserialize should not be in derive attribute"
  );
  assert!(
    impl_part.contains("impl < 'de > serde :: Deserialize < 'de > for Status"),
    "Should implement Deserialize manually"
  );

  assert!(impl_part.contains("\"active\" => Ok (Status :: Active)"));
  assert!(impl_part.contains("\"in-progress\" => Ok (Status :: InProgress)"));
}
