use oas3::{
  Spec,
  spec::{MediaType, ObjectOrReference, ObjectSchema, Operation, Response},
};

use super::{SchemaConverter, cache::SharedSchemaCache};
use crate::generator::{
  ast::{
    ContentCategory, Documentation, EnumToken, EnumVariantToken, MethodNameToken, ResponseEnumDef, ResponseMediaType,
    ResponseVariant, RustPrimitive, StatusCodeToken, StructMethod, StructMethodKind, TypeRef,
    status_code_to_variant_name,
  },
  converter::SchemaExt as _,
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

    let status_code = status_code_str
      .parse::<StatusCodeToken>()
      .unwrap_or(StatusCodeToken::Default);
    let variant_name = status_code_to_variant_name(status_code, &response);
    let mut media_types =
      extract_all_media_type_schemas(schema_converter, &response, path, status_code, schema_cache).unwrap_or_default();

    if media_types.is_empty() {
      media_types.push(ResponseMediaType::new("application/json"));
    }

    let split_variants =
      split_mixed_content_variants(status_code, &variant_name, response.description.as_ref(), &media_types);
    variants.extend(split_variants);
  }

  let variants = normalize_response_variants(variants);
  if variants.is_empty() {
    return None;
  }

  Some(
    ResponseEnumDef::builder()
      .name(EnumToken::new(&base_name))
      .docs(Documentation::from_lines([format!(
        "Response types for {}",
        operation.operation_id.as_ref()?
      )]))
      .variants(variants)
      .build(),
  )
}

fn split_mixed_content_variants(
  status_code: StatusCodeToken,
  variant_name: &EnumVariantToken,
  description: Option<&String>,
  media_types: &[ResponseMediaType],
) -> Vec<ResponseVariant> {
  let typed_media: Vec<_> = media_types.iter().filter(|m| m.schema_type.is_some()).collect();

  if typed_media.is_empty() {
    return vec![ResponseVariant {
      status_code,
      variant_name: variant_name.clone(),
      description: description.cloned(),
      media_types: media_types.to_vec(),
      schema_type: None,
    }];
  }

  let event_stream_media = typed_media.iter().find(|m| m.category == ContentCategory::EventStream);
  let non_stream_media: Vec<_> = typed_media
    .iter()
    .filter(|m| m.category != ContentCategory::EventStream)
    .collect();

  let has_both = event_stream_media.is_some() && !non_stream_media.is_empty();

  if has_both {
    let json_schema = non_stream_media.first().and_then(|m| m.schema_type.clone());
    let non_stream_media_types: Vec<_> = media_types
      .iter()
      .filter(|m| m.category != ContentCategory::EventStream)
      .cloned()
      .collect();

    let stream_media = event_stream_media.unwrap();
    let inner_type = stream_media.schema_type.as_ref().unwrap();
    let stream_schema = TypeRef::new(format!("oas3_gen_support::EventStream<{}>", inner_type.to_rust_type()));
    let stream_media_types: Vec<_> = media_types
      .iter()
      .filter(|m| m.category == ContentCategory::EventStream)
      .cloned()
      .collect();

    let stream_variant_name = EnumVariantToken::new(format!("{variant_name}EventStream"));

    vec![
      ResponseVariant {
        status_code,
        variant_name: variant_name.clone(),
        description: description.cloned(),
        media_types: non_stream_media_types,
        schema_type: json_schema,
      },
      ResponseVariant::builder()
        .status_code(status_code)
        .variant_name(stream_variant_name)
        .maybe_description(description.cloned())
        .media_types(stream_media_types)
        .schema_type(stream_schema)
        .build(),
    ]
  } else if let Some(stream_media) = event_stream_media {
    let inner_type = stream_media.schema_type.as_ref().unwrap();
    let stream_schema = TypeRef::new(format!("oas3_gen_support::EventStream<{}>", inner_type.to_rust_type()));

    vec![
      ResponseVariant::builder()
        .status_code(status_code)
        .variant_name(variant_name.clone())
        .maybe_description(description.cloned())
        .media_types(media_types.to_vec())
        .schema_type(stream_schema)
        .build(),
    ]
  } else {
    let schema_type = non_stream_media.first().and_then(|m| m.schema_type.clone());

    vec![
      ResponseVariant::builder()
        .status_code(status_code)
        .variant_name(variant_name.clone())
        .maybe_description(description.cloned())
        .media_types(media_types.to_vec())
        .maybe_schema_type(schema_type)
        .build(),
    ]
  }
}

fn extract_all_media_type_schemas(
  schema_converter: &SchemaConverter,
  response: &Response,
  path: &str,
  status_code: StatusCodeToken,
  schema_cache: &mut SharedSchemaCache,
) -> anyhow::Result<Vec<ResponseMediaType>> {
  let mut result = vec![];

  for (content_type, media_type) in &response.content {
    let schema_type = extract_single_media_type_schema(
      schema_converter,
      content_type,
      media_type,
      path,
      status_code,
      schema_cache,
    )?;

    result.push(ResponseMediaType::with_schema(content_type, schema_type));
  }

  Ok(result)
}

fn extract_single_media_type_schema(
  schema_converter: &SchemaConverter,
  content_type: &str,
  media_type: &MediaType,
  path: &str,
  status_code: StatusCodeToken,
  schema_cache: &mut SharedSchemaCache,
) -> anyhow::Result<Option<TypeRef>> {
  let category = ContentCategory::from_content_type(content_type);

  if category == ContentCategory::Binary && status_code.is_success() {
    return Ok(Some(TypeRef::new(RustPrimitive::Bytes)));
  }

  let Some(schema_ref) = media_type.schema.as_ref() else {
    return Ok(None);
  };

  match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => {
      Ok(SchemaRegistry::extract_ref_name(ref_path).map(|name| TypeRef::new(to_rust_type_name(&name))))
    }
    ObjectOrReference::Object(inline_schema) => {
      resolve_inline_response_schema(schema_converter, inline_schema, path, status_code, schema_cache)
    }
  }
}

fn resolve_inline_response_schema(
  schema_converter: &SchemaConverter,
  inline_schema: &ObjectSchema,
  path: &str,
  status_code: StatusCodeToken,
  schema_cache: &mut SharedSchemaCache,
) -> anyhow::Result<Option<TypeRef>> {
  let has_compound_schema = inline_schema.has_intersection() || inline_schema.has_union();

  if inline_schema.properties.is_empty() && inline_schema.schema_type.is_none() && !has_compound_schema {
    return Ok(None);
  }

  if inline_schema.properties.is_empty()
    && !has_compound_schema
    && let Ok(primitive_ref) = schema_converter.resolve_type(inline_schema)
    && !matches!(primitive_ref.base_type, RustPrimitive::Custom(_))
  {
    return Ok(Some(primitive_ref));
  }

  let effective_for_naming = if has_compound_schema {
    schema_converter.merge_inline_all_of(inline_schema)
  } else {
    inline_schema.clone()
  };

  let base_name = naming::infer_name_from_context(&effective_for_naming, path, status_code.as_str());
  let Some(output) = schema_converter.convert_inline_schema(inline_schema, &base_name, schema_cache)? else {
    return Ok(None);
  };

  Ok(Some(TypeRef::new(output.type_name)))
}

fn normalize_response_variants(mut variants: Vec<ResponseVariant>) -> Vec<ResponseVariant> {
  if variants.is_empty() {
    return variants;
  }

  let has_default = variants.iter().any(|v| v.status_code.is_default());
  if !has_default {
    variants.push(
      ResponseVariant::builder()
        .variant_name(EnumVariantToken::from_raw(DEFAULT_RESPONSE_VARIANT))
        .description(DEFAULT_RESPONSE_DESCRIPTION.to_string())
        .media_types(vec![ResponseMediaType::new("application/json")])
        .build(),
    );
  }

  variants
}

pub(crate) fn build_parse_response_method(response_enum: &EnumToken, variants: &[ResponseVariant]) -> StructMethod {
  StructMethod::builder()
    .name(MethodNameToken::from_raw("parse_response"))
    .docs(Documentation::from_lines([
      "Parse the HTTP response into the response enum.",
    ]))
    .kind(StructMethodKind::ParseResponse {
      response_enum: response_enum.clone(),
      variants: variants.to_vec(),
    })
    .build()
}
