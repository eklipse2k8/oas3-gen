use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Response},
};

use super::{SchemaConverter, cache::SharedSchemaCache};
use crate::generator::{
  ast::{ResponseEnumDef, ResponseVariant, RustPrimitive, StructKind, StructMethod, StructMethodKind, TypeRef},
  naming::{
    constants::{DEFAULT_RESPONSE_DESCRIPTION, DEFAULT_RESPONSE_VARIANT},
    identifiers::to_rust_type_name,
    inference as naming,
    status_codes::status_code_to_variant_name,
  },
  schema_registry::SchemaRegistry,
};

pub(crate) fn build_response_enum(
  schema_converter: &SchemaConverter,
  spec: &Spec,
  name: &str,
  request_type: Option<&String>,
  operation: &Operation,
  path: &str,
  schema_cache: &mut SharedSchemaCache,
) -> Option<ResponseEnumDef> {
  let responses = operation.responses.as_ref()?;

  let mut variants = vec![];
  let base_name = to_rust_type_name(name);

  for (status_code, resp_ref) in responses {
    let Ok(response) = resp_ref.resolve(spec) else {
      continue;
    };

    let variant_name = status_code_to_variant_name(status_code, &response);
    let (schema_type, content_type) =
      extract_response_schema_info(schema_converter, &response, path, status_code, schema_cache)
        .ok()
        .unwrap_or((None, None));

    variants.push(ResponseVariant {
      status_code: status_code.clone(),
      variant_name,
      description: response.description.clone(),
      schema_type,
      content_type,
    });
  }

  if variants.is_empty() {
    return None;
  }

  let has_default = variants.iter().any(|v| v.status_code == "default");
  if !has_default {
    variants.push(ResponseVariant {
      status_code: "default".to_string(),
      variant_name: DEFAULT_RESPONSE_VARIANT.to_string(),
      description: Some(DEFAULT_RESPONSE_DESCRIPTION.to_string()),
      schema_type: None,
      content_type: None,
    });
  }

  Some(ResponseEnumDef {
    name: base_name,
    docs: vec![format!("/// Response types for {}", operation.operation_id.as_ref()?)],
    variants,
    request_type: request_type.map_or_else(String::new, Clone::clone),
  })
}

fn extract_response_schema_info(
  schema_converter: &SchemaConverter,
  response: &Response,
  path: &str,
  status_code: &str,
  schema_cache: &mut SharedSchemaCache,
) -> anyhow::Result<(Option<TypeRef>, Option<String>)> {
  let Some((content_type, media_type)) = response.content.iter().next() else {
    return Ok((None, None));
  };
  let Some(schema_ref) = media_type.schema.as_ref() else {
    return Ok((None, Some(content_type.clone())));
  };

  match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => {
      let Some(schema_name) = SchemaRegistry::extract_ref_name(ref_path) else {
        return Ok((None, Some(content_type.clone())));
      };
      Ok((
        Some(TypeRef::new(to_rust_type_name(&schema_name))),
        Some(content_type.clone()),
      ))
    }
    ObjectOrReference::Object(inline_schema) => {
      if inline_schema.properties.is_empty() && inline_schema.schema_type.is_none() {
        return Ok((None, Some(content_type.clone())));
      }

      if inline_schema.properties.is_empty()
        && let Ok(type_ref) = schema_converter.schema_to_type_ref(inline_schema)
        && !matches!(type_ref.base_type, RustPrimitive::Custom(_))
      {
        return Ok((Some(type_ref), Some(content_type.clone())));
      }

      let cached_type_name = schema_cache.get_type_name(inline_schema)?;

      let rust_type_name = if let Some(name) = cached_type_name {
        name
      } else {
        let base_name = naming::infer_name_from_context(inline_schema, path, status_code);
        let unique_name = schema_cache.make_unique_name(&base_name);

        let result = schema_converter.convert_struct(
          &unique_name,
          inline_schema,
          Some(StructKind::Schema),
          Some(schema_cache),
        )?;

        schema_cache.register_type(inline_schema, &unique_name, result.inline_types, result.result)?
      };

      Ok((Some(TypeRef::new(rust_type_name)), Some(content_type.clone())))
    }
  }
}

pub(crate) fn build_parse_response_method(response_enum_name: &str, variants: &[ResponseVariant]) -> StructMethod {
  StructMethod {
    name: "parse_response".to_string(),
    docs: vec!["/// Parse the HTTP response into the response enum.".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: response_enum_name.to_string(),
      variants: variants.to_vec(),
    },
    attrs: vec![],
  }
}
