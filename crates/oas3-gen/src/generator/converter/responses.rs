use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Response},
};

use super::{SchemaConverter, cache::SharedSchemaCache};
use crate::generator::{
  ast::{
    ContentCategory, EnumToken, EnumVariantToken, MethodNameToken, ResponseEnumDef, ResponseVariant, RustPrimitive, StatusCodeToken,
    StructKind, StructMethod, StructMethodKind, TypeRef, status_code_to_variant_name,
  },
  naming::{
    constants::{DEFAULT_RESPONSE_DESCRIPTION, DEFAULT_RESPONSE_VARIANT},
    identifiers::to_rust_type_name,
    inference as naming,
  },
  schema_registry::SchemaRegistry,
};

pub(crate) fn build_response_enum(
  schema_converter: &SchemaConverter,
  spec: &Spec,
  name: &str,
  operation: &Operation,
  path: &str,
  schema_cache: &mut SharedSchemaCache,
) -> Option<ResponseEnumDef> {
  let responses = operation.responses.as_ref()?;

  let mut variants = vec![];
  let base_name = to_rust_type_name(name);

  for (status_code_str, resp_ref) in responses {
    let Ok(response) = resp_ref.resolve(spec) else {
      continue;
    };

    let status_code = StatusCodeToken::from_openapi(status_code_str);
    let variant_name = status_code_to_variant_name(status_code, &response);
    let (schema_type, content_category) =
      extract_response_schema_info(schema_converter, &response, path, status_code, schema_cache)
        .ok()
        .unwrap_or((None, ContentCategory::Json));

    variants.push(ResponseVariant {
      status_code,
      variant_name,
      description: response.description.clone(),
      schema_type,
      content_category,
    });
  }

  if variants.is_empty() {
    return None;
  }

  let has_default = variants.iter().any(|v| v.status_code.is_default());
  if !has_default {
    variants.push(ResponseVariant {
      status_code: StatusCodeToken::Default,
      variant_name: EnumVariantToken::new(DEFAULT_RESPONSE_VARIANT),
      description: Some(DEFAULT_RESPONSE_DESCRIPTION.to_string()),
      schema_type: None,
      content_category: ContentCategory::Json,
    });
  }

  Some(ResponseEnumDef {
    name: EnumToken::new(&base_name),
    docs: vec![format!("/// Response types for {}", operation.operation_id.as_ref()?)],
    variants,
    request_type: None,
  })
}

fn extract_response_schema_info(
  schema_converter: &SchemaConverter,
  response: &Response,
  path: &str,
  status_code: StatusCodeToken,
  schema_cache: &mut SharedSchemaCache,
) -> anyhow::Result<(Option<TypeRef>, ContentCategory)> {
  let Some((content_type, media_type)) = response.content.iter().next() else {
    return Ok((None, ContentCategory::Json));
  };
  let content_category = ContentCategory::from_content_type(content_type);
  let Some(schema_ref) = media_type.schema.as_ref() else {
    return Ok((None, content_category));
  };

  match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => {
      let Some(schema_name) = SchemaRegistry::extract_ref_name(ref_path) else {
        return Ok((None, content_category));
      };
      Ok((Some(TypeRef::new(to_rust_type_name(&schema_name))), content_category))
    }
    ObjectOrReference::Object(inline_schema) => {
      if inline_schema.properties.is_empty() && inline_schema.schema_type.is_none() {
        return Ok((None, content_category));
      }

      if inline_schema.properties.is_empty()
        && let Ok(type_ref) = schema_converter.schema_to_type_ref(inline_schema)
        && !matches!(type_ref.base_type, RustPrimitive::Custom(_))
      {
        return Ok((Some(type_ref), content_category));
      }

      let cached_type_name = schema_cache.get_type_name(inline_schema)?;

      let rust_type_name = if let Some(name) = cached_type_name {
        name
      } else {
        let base_name = naming::infer_name_from_context(inline_schema, path, status_code.as_str());
        let unique_name = schema_cache.make_unique_name(&base_name);

        let result = schema_converter.convert_struct(
          &unique_name,
          inline_schema,
          Some(StructKind::Schema),
          Some(schema_cache),
        )?;

        schema_cache.register_type(inline_schema, &unique_name, result.inline_types, result.result)?
      };

      Ok((Some(TypeRef::new(rust_type_name)), content_category))
    }
  }
}

pub(crate) fn build_parse_response_method(response_enum: &EnumToken, variants: &[ResponseVariant]) -> StructMethod {
  StructMethod {
    name: MethodNameToken::new("parse_response"),
    docs: vec!["/// Parse the HTTP response into the response enum.".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: response_enum.clone(),
      variants: variants.to_vec(),
    },
    attrs: vec![],
  }
}
