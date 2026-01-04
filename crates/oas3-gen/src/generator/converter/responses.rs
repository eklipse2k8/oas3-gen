use std::rc::Rc;

use oas3::spec::{MediaType, ObjectOrReference, ObjectSchema, Operation, Response};

use super::{ConverterContext, TypeResolver, TypeUsageRecorder};
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
    inference::InferenceExt,
    responses as naming_responses,
  },
  schema_registry::SchemaRegistry,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ResponseMetadata {
  pub(crate) type_name: Option<String>,
  pub(crate) media_types: Vec<ResponseMediaType>,
}

/// Converts OpenAPI responses into Rust enum definitions.
///
/// Handles status codes, media types, and schema resolution for each response.
#[derive(Debug, Clone)]
pub(crate) struct ResponseConverter {
  context: Rc<ConverterContext>,
}

impl ResponseConverter {
  /// Creates a new response converter.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    Self { context }
  }

  /// Builds a response enum for an operation.
  ///
  /// Returns `None` if the operation has no responses or only empty responses.
  pub(crate) fn build_enum(&self, name: &str, operation: &Operation, path: &str) -> Option<ResponseEnumDef> {
    let spec = self.context.graph().spec();
    let responses = operation.responses.as_ref()?;
    let base_name = to_rust_type_name(name);

    let mut variants = vec![];
    for (status_str, resp_ref) in responses {
      let Ok(response) = resp_ref.resolve(spec) else {
        continue;
      };

      let status_code = status_str.parse().unwrap_or(StatusCodeToken::Default);
      let variant_name = status_code_to_variant_name(status_code, &response);

      let mut media_types = self
        .extract_media_types(&response, path, status_code)
        .unwrap_or_default();

      if media_types.is_empty() {
        media_types.push(ResponseMediaType::new("application/json"));
      }

      variants.extend(split_by_content_type(
        status_code,
        &variant_name,
        response.description.as_ref(),
        &media_types,
      ));
    }

    let variants = ensure_default_variant(variants);
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

  /// Extracts response metadata for operation info.
  ///
  /// Gathers type names and media types, marking them for usage tracking.
  pub(crate) fn extract_metadata(&self, operation: &Operation, usage: &mut TypeUsageRecorder) -> ResponseMetadata {
    let spec = self.context.graph().spec();
    let type_name = naming_responses::extract_response_type_name(spec, operation);
    let response_types = naming_responses::extract_all_response_types(spec, operation);

    let media_types: Vec<_> = naming_responses::extract_all_response_content_types(spec, operation)
      .into_iter()
      .map(|ct| ResponseMediaType::new(&ct))
      .collect();

    let media_types = if media_types.is_empty() {
      vec![ResponseMediaType::new("application/json")]
    } else {
      media_types
    };

    if let Some(ref name) = type_name {
      usage.mark_response(name);
    }
    usage.mark_response_iter(&response_types.success);
    usage.mark_response_iter(&response_types.error);

    ResponseMetadata { type_name, media_types }
  }

  fn extract_media_types(
    &self,
    response: &Response,
    path: &str,
    status_code: StatusCodeToken,
  ) -> anyhow::Result<Vec<ResponseMediaType>> {
    response
      .content
      .iter()
      .map(|(content_type, media_type)| {
        let schema_type = self.resolve_media_schema(content_type, media_type, path, status_code)?;
        Ok(ResponseMediaType::with_schema(content_type, schema_type))
      })
      .collect()
  }

  fn resolve_media_schema(
    &self,
    content_type: &str,
    media_type: &MediaType,
    path: &str,
    status_code: StatusCodeToken,
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
        Ok(SchemaRegistry::parse_ref(ref_path).map(|name| TypeRef::new(to_rust_type_name(&name))))
      }
      ObjectOrReference::Object(schema) => self.resolve_inline_response_schema(schema, path, status_code),
    }
  }

  fn resolve_inline_response_schema(
    &self,
    schema: &ObjectSchema,
    path: &str,
    status_code: StatusCodeToken,
  ) -> anyhow::Result<Option<TypeRef>> {
    let has_compound = schema.has_intersection() || schema.has_union();

    if schema.properties.is_empty() && schema.schema_type.is_none() && !has_compound {
      return Ok(None);
    }

    let type_resolver = TypeResolver::new(self.context.clone());

    if schema.properties.is_empty()
      && !has_compound
      && let Ok(primitive) = type_resolver.resolve_type(schema)
      && !matches!(primitive.base_type, RustPrimitive::Custom(_))
    {
      return Ok(Some(primitive));
    }

    let effective = if has_compound {
      self.context.graph().merge_all_of(schema)
    } else {
      schema.clone()
    };

    let base_name = effective.infer_name_from_context(path, status_code.as_str());
    let Some(output) = type_resolver.try_inline_schema(schema, &base_name)? else {
      return Ok(None);
    };

    Ok(Some(TypeRef::new(output.type_name)))
  }
}

/// Builds the `parse_response` method for request structs.
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

fn split_by_content_type(
  status_code: StatusCodeToken,
  variant_name: &EnumVariantToken,
  description: Option<&String>,
  media_types: &[ResponseMediaType],
) -> Vec<ResponseVariant> {
  let typed: Vec<_> = media_types.iter().filter(|m| m.schema_type.is_some()).collect();

  if typed.is_empty() {
    return vec![
      ResponseVariant::builder()
        .status_code(status_code)
        .variant_name(variant_name.clone())
        .maybe_description(description.cloned())
        .media_types(media_types.to_vec())
        .build(),
    ];
  }

  let stream = typed.iter().find(|m| m.category == ContentCategory::EventStream);
  let non_stream: Vec<_> = typed
    .iter()
    .filter(|m| m.category != ContentCategory::EventStream)
    .collect();

  match (stream, non_stream.is_empty()) {
    // Both stream and non-stream: create two variants
    (Some(s), false) => {
      let json_schema = non_stream.first().and_then(|m| m.schema_type.clone());
      let non_stream_types: Vec<_> = media_types
        .iter()
        .filter(|m| m.category != ContentCategory::EventStream)
        .cloned()
        .collect();

      let inner = s.schema_type.as_ref().unwrap();
      let stream_type = TypeRef::new(format!("oas3_gen_support::EventStream<{}>", inner.to_rust_type()));
      let stream_types: Vec<_> = media_types
        .iter()
        .filter(|m| m.category == ContentCategory::EventStream)
        .cloned()
        .collect();

      vec![
        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(variant_name.clone())
          .maybe_description(description.cloned())
          .media_types(non_stream_types)
          .maybe_schema_type(json_schema)
          .build(),
        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(EnumVariantToken::new(format!("{variant_name}EventStream")))
          .maybe_description(description.cloned())
          .media_types(stream_types)
          .schema_type(stream_type)
          .build(),
      ]
    }
    // Only stream
    (Some(s), true) => {
      let inner = s.schema_type.as_ref().unwrap();
      let stream_type = TypeRef::new(format!("oas3_gen_support::EventStream<{}>", inner.to_rust_type()));
      vec![
        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(variant_name.clone())
          .maybe_description(description.cloned())
          .media_types(media_types.to_vec())
          .schema_type(stream_type)
          .build(),
      ]
    }
    // Only non-stream
    (None, _) => {
      let schema = non_stream.first().and_then(|m| m.schema_type.clone());
      vec![
        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(variant_name.clone())
          .maybe_description(description.cloned())
          .media_types(media_types.to_vec())
          .maybe_schema_type(schema)
          .build(),
      ]
    }
  }
}

fn ensure_default_variant(mut variants: Vec<ResponseVariant>) -> Vec<ResponseVariant> {
  if variants.is_empty() {
    return variants;
  }

  if !variants.iter().any(|v| v.status_code.is_default()) {
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
