use std::collections::BTreeSet;

use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Operation, Response},
};

use super::{SchemaConverter, cache::SharedSchemaCache, links::LinkConverter};
use crate::generator::{
  ast::{
    ContentCategory, EnumToken, EnumVariantToken, MethodNameToken, ResolvedLink, ResponseEnumDef, ResponseVariant,
    ResponseVariantLinks, RustPrimitive, StatusCodeToken, StructKind, StructMethod, StructMethodKind, StructToken,
    TypeRef, status_code_to_variant_name,
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

  let link_converter = LinkConverter::new(spec);
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

    let link_defs = link_converter.extract_links_from_response(&response);

    let links = if link_defs.is_empty() {
      None
    } else {
      let links_struct_name = format!("{base_name}{variant_name}Links");
      let resolved_links: Vec<ResolvedLink> = link_defs
        .iter()
        .map(|link_def| {
          let target_request_name = format!("{}Request", to_rust_type_name(&link_def.target_operation_id));
          ResolvedLink {
            link_def: link_def.clone(),
            target_request_type: StructToken::from_raw(&target_request_name),
          }
        })
        .collect();

      let response_body_fields = extract_response_body_fields(spec, &response);

      Some(ResponseVariantLinks {
        links_struct_name: StructToken::from_raw(&links_struct_name),
        resolved_links,
        response_body_fields,
      })
    };

    variants.push(ResponseVariant {
      status_code,
      variant_name,
      description: response.description.clone(),
      schema_type,
      content_category,
      links,
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
      links: None,
    });
  }

  Some(ResponseEnumDef {
    name: EnumToken::new(&base_name),
    docs: vec![format!("Response types for {}", operation.operation_id.as_ref()?)],
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

  if content_category == ContentCategory::Binary && status_code.is_success() {
    return Ok((Some(TypeRef::new(RustPrimitive::Bytes)), content_category));
  }

  let Some(schema_ref) = media_type.schema.as_ref() else {
    return Ok((None, content_category));
  };

  let type_ref = match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => {
      SchemaRegistry::extract_ref_name(ref_path).map(|name| TypeRef::new(to_rust_type_name(&name)))
    }
    ObjectOrReference::Object(inline_schema) => {
      if inline_schema.properties.is_empty() && inline_schema.schema_type.is_none() {
        return Ok((None, content_category));
      }

      if inline_schema.properties.is_empty()
        && let Ok(primitive_ref) = schema_converter.resolve_type(inline_schema)
        && !matches!(primitive_ref.base_type, RustPrimitive::Custom(_))
      {
        return Ok((Some(primitive_ref), content_category));
      }

      let type_name = if let Some(cached) = schema_cache.get_type_name(inline_schema)? {
        cached
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

      Some(TypeRef::new(type_name))
    }
  };

  Ok((type_ref, content_category))
}

pub(crate) fn build_parse_response_method(response_enum: &EnumToken, variants: &[ResponseVariant]) -> StructMethod {
  StructMethod {
    name: MethodNameToken::new("parse_response"),
    docs: vec!["Parse the HTTP response into the response enum.".to_string()],
    kind: StructMethodKind::ParseResponse {
      response_enum: response_enum.clone(),
      variants: variants.to_vec(),
    },
    attrs: vec![],
  }
}

fn extract_response_body_fields(spec: &Spec, response: &Response) -> BTreeSet<String> {
  let Some((_, media_type)) = response.content.iter().next() else {
    return BTreeSet::new();
  };

  let Some(schema_ref) = media_type.schema.as_ref() else {
    return BTreeSet::new();
  };

  match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => extract_fields_from_ref(spec, ref_path),
    ObjectOrReference::Object(inline_schema) => extract_fields_from_schema(inline_schema),
  }
}

fn extract_fields_from_ref(spec: &Spec, ref_path: &str) -> BTreeSet<String> {
  let Some(name) = ref_path.strip_prefix("#/components/schemas/") else {
    return BTreeSet::new();
  };

  let Some(components) = &spec.components else {
    return BTreeSet::new();
  };

  let Some(schema_ref) = components.schemas.get(name) else {
    return BTreeSet::new();
  };

  match schema_ref {
    ObjectOrReference::Object(schema) => extract_fields_from_schema(schema),
    ObjectOrReference::Ref { .. } => BTreeSet::new(),
  }
}

fn extract_fields_from_schema(schema: &ObjectSchema) -> BTreeSet<String> {
  schema.properties.keys().cloned().collect()
}
