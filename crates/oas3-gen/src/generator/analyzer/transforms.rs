use std::collections::BTreeMap;

use super::type_usage::TypeUsage;
use crate::generator::ast::RustType;

pub(crate) fn update_derives_from_usage(rust_types: &mut [RustType], type_usage: &BTreeMap<String, TypeUsage>) {
  for rust_type in rust_types {
    if let RustType::Struct(def) = rust_type {
      let type_name = &def.name;
      def.derives.retain(|d| d != "Serialize" && d != "Deserialize");

      let needs_serialization = if let Some(usage) = type_usage.get(type_name) {
        match usage {
          TypeUsage::RequestOnly => {
            def.derives.push("Serialize".to_string());
            true
          }
          TypeUsage::ResponseOnly => {
            def.derives.push("Deserialize".to_string());
            def.derives.retain(|d| d != "validator::Validate");
            for field in &mut def.fields {
              field.validation_attrs.clear();
            }
            false
          }
          TypeUsage::Bidirectional => {
            def.derives.push("Serialize".to_string());
            def.derives.push("Deserialize".to_string());
            true
          }
        }
      } else {
        def.derives.push("Serialize".to_string());
        def.derives.push("Deserialize".to_string());
        true
      };

      if needs_serialization
        && has_nullable_fields(&def.fields)
        && def.kind != crate::generator::ast::StructKind::OperationRequest
      {
        let attr = "oas3_gen_support::skip_serializing_none".to_string();
        if !def.outer_attrs.contains(&attr) {
          def.outer_attrs.push(attr);
        }
      }
    }
  }
}

fn has_nullable_fields(fields: &[crate::generator::ast::FieldDef]) -> bool {
  fields.iter().any(|field| field.rust_type.nullable)
}
