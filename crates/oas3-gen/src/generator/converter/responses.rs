use std::rc::Rc;

use http::header::CONTENT_TYPE;
use indexmap::IndexMap;
use itertools::Itertools;
use oas3::spec::{MediaType, ObjectSchema, Operation, Response, Schema};

use super::{
  ConverterContext, SerdeUsageRecorder, TypeResolver, fields::FieldConverter, inline_resolver::InlineTypeResolver,
};
use crate::{
  generator::{
    ast::{
      ContentCategory, Documentation, FieldDef, FieldNameToken, ResponseMediaType, ResponseVariant, RustPrimitive,
      StatusCodeToken, TypeRef,
    },
    naming::{
      constants::{DEFAULT_MEDIA_TYPE, RESPONSE_HEADER_PARENT_SUFFIX},
      identifiers::to_rust_type_name,
      responses as naming_responses,
    },
  },
  utils::{SchemaExt as _, SchemaInspect, parse_schema_ref_path, schema_ext::SchemaExtIters},
};

/// Extracted metadata about operation responses for code generation.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResponseMetadata {
  pub(crate) type_name: Option<String>,
  pub(crate) media_types: Vec<ResponseMediaType>,
}

/// Response metadata bundled with type usage data.
#[derive(Debug, Clone)]
pub(crate) struct ResponseMetadataOutput {
  pub(crate) metadata: ResponseMetadata,
  pub(crate) usage: SerdeUsageRecorder,
}

/// Converts the responses an operation declares into response variants.
///
/// Handles status codes, media types, and schema resolution for each response.
/// The shared response enum those variants map onto is assembled in
/// postprocessing, once every operation is known.
#[derive(Debug, Clone)]
pub(crate) struct ResponseConverter {
  type_resolver: TypeResolver,
  inline_resolver: InlineTypeResolver,
  field_converter: FieldConverter,
  context: Rc<ConverterContext>,
}

impl ResponseConverter {
  /// Creates a new response converter.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let type_resolver = TypeResolver::new(context.clone());
    let inline_resolver = InlineTypeResolver::new(context.clone());
    let field_converter = FieldConverter::new(&context);
    Self {
      type_resolver,
      inline_resolver,
      field_converter,
      context,
    }
  }

  /// Collects the responses an operation declares, one variant per status code
  /// and distinct body schema, each carrying the headers its response declares.
  ///
  /// Returns `None` if the operation declares no responses. Variant names are the
  /// status-code names used by the shared response enum. `base_name` prefixes the
  /// names of inline types generated from header schemas.
  pub(crate) fn build_variants(
    &self,
    operation: &Operation,
    path: &str,
    base_name: &str,
  ) -> anyhow::Result<Option<Vec<ResponseVariant>>> {
    let spec = self.context.graph().spec();
    let Some(responses) = operation.responses.as_ref() else {
      return Ok(None);
    };
    let header_parent = format!("{base_name}{RESPONSE_HEADER_PARENT_SUFFIX}");

    let variants = responses
      .iter()
      .resolve_all(spec)
      .map(|(status_str, response)| {
        let status_code = status_str
          .parse::<StatusCodeToken>()
          .unwrap_or(StatusCodeToken::Default);
        let media_types = Self::with_default_media_type(
          self
            .extract_media_types(&response, path, status_code)
            .unwrap_or_default(),
        );
        let headers = self.header_fields(&response, &header_parent)?;

        anyhow::Ok(Self::split_variants_by_schema(status_code, &media_types, headers))
      })
      .flatten_ok()
      .filter_ok(|variant| !(variant.status_code.is_default() && variant.schema_type.is_none()))
      .collect::<anyhow::Result<Vec<_>>>()?;

    Ok((!variants.is_empty()).then_some(variants))
  }

  /// Converts the headers a response declares into fields of a response headers struct.
  ///
  /// `Content-Type` is skipped, as the OpenAPI specification requires. A header
  /// without a schema is read as a `String`.
  fn header_fields(&self, response: &Response, parent_name: &str) -> anyhow::Result<Vec<FieldDef>> {
    let spec = self.context.graph().spec();

    response
      .headers
      .iter()
      .resolve_all(spec)
      .filter(|(name, _)| !name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()))
      .map(|(name, header)| {
        let required = header.required.unwrap_or(false);
        let (rust_type, schema_docs) = match header.schema.as_ref() {
          Some(schema_ref) => {
            let resolved = self
              .field_converter
              .resolve_with_metadata(parent_name, name, schema_ref, required)?;
            (resolved.type_ref, resolved.schema.description)
          }
          None => (TypeRef::new(RustPrimitive::String), None),
        };

        Ok(
          FieldDef::builder()
            .name(FieldNameToken::from_raw(name))
            .docs(Documentation::from_optional(
              header.description.as_ref().or(schema_docs.as_ref()),
            ))
            .rust_type(if required { rust_type } else { rust_type.with_option() })
            .deprecated(header.deprecated.unwrap_or(false))
            .original_name(name.clone())
            .build(),
        )
      })
      .collect()
  }

  /// Extracts response metadata for operation info.
  ///
  /// Gathers type names and media types, returning usage data.
  pub(crate) fn extract_metadata(&self, operation: &Operation) -> ResponseMetadataOutput {
    let spec = self.context.graph().spec();
    let type_name = naming_responses::extract_response_type_name(spec, operation);
    let response_types = naming_responses::extract_all_response_types(spec, operation);

    let media_types = Self::with_default_media_type(
      naming_responses::extract_all_response_content_types(spec, operation)
        .into_iter()
        .map(|ct| ResponseMediaType::new(&ct))
        .collect(),
    );

    let mut usage = SerdeUsageRecorder::new();
    if let Some(ref name) = type_name {
      usage.mark_response(name);
    }
    usage.mark_response_iter(&response_types.success);
    usage.mark_response_iter(&response_types.error);

    ResponseMetadataOutput {
      metadata: ResponseMetadata { type_name, media_types },
      usage,
    }
  }

  /// Extracts media type information from a response definition.
  ///
  /// Resolves schemas for each content type and maps binary responses
  /// to `Bytes` for success status codes.
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

  /// Resolves the schema type for a specific media type in a response.
  ///
  /// Returns `Bytes` for binary content types on success responses,
  /// resolves `$ref` schemas to type references, and creates inline
  /// types for anonymous schemas.
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

    if matches!(schema_ref, Schema::Boolean(_)) {
      return Ok(Some(TypeRef::new(RustPrimitive::Value)));
    }

    if let Some(ref_path) = schema_ref.ref_path() {
      return Ok(parse_schema_ref_path(ref_path).map(|name| TypeRef::new(to_rust_type_name(&name))));
    }

    if let Some(schema) = schema_ref.as_inline() {
      return self.resolve_inline_schema(schema, path, status_code);
    }

    Ok(None)
  }

  /// Resolves an inline response schema to a type reference.
  ///
  /// Returns `None` for empty schemas. For primitive types without
  /// properties, returns the primitive directly. For complex types,
  /// creates a named type via the inline resolver.
  fn resolve_inline_schema(
    &self,
    schema: &ObjectSchema,
    path: &str,
    status_code: StatusCodeToken,
  ) -> anyhow::Result<Option<TypeRef>> {
    let has_compound = schema.has_intersection() || schema.has_union();

    if schema.properties.is_empty() && schema.schema_type.is_none() && !has_compound {
      return Ok(None);
    }

    if schema.properties.is_empty()
      && !has_compound
      && let Ok(primitive) = self.type_resolver.resolve_type(schema)
      && !matches!(primitive.base_type, RustPrimitive::Custom(_))
    {
      return Ok(Some(primitive));
    }

    let base_name = schema.infer_name_from_context(path, status_code.as_str());
    let Some(output) = self.inline_resolver.try_inline_schema(schema, &base_name)? else {
      return Ok(None);
    };

    Ok(Some(TypeRef::new(output.result)))
  }

  /// Ensures at least one media type exists, defaulting to `application/json`.
  fn with_default_media_type(media_types: Vec<ResponseMediaType>) -> Vec<ResponseMediaType> {
    if media_types.is_empty() {
      vec![ResponseMediaType::new(DEFAULT_MEDIA_TYPE)]
    } else {
      media_types
    }
  }

  fn split_variants_by_schema(
    status_code: StatusCodeToken,
    media_types: &[ResponseMediaType],
    headers: Vec<FieldDef>,
  ) -> Vec<ResponseVariant> {
    let grouped = Self::group_media_types_by_schema(media_types);

    if grouped.is_empty() {
      return vec![
        ResponseVariant::builder()
          .status_code(status_code)
          .media_types(media_types.to_vec())
          .headers(headers)
          .build(),
      ];
    }

    let count = grouped.len();
    grouped
      .into_iter()
      .zip(std::iter::repeat_n(headers, count))
      .map(|((schema_key, types), headers)| {
        ResponseVariant::builder()
          .status_code(status_code)
          .media_types(types)
          .maybe_schema_type(Some(TypeRef::new(schema_key)))
          .headers(headers)
          .build()
      })
      .collect()
  }

  /// Groups media types by their schema type for variant splitting.
  fn group_media_types_by_schema(media_types: &[ResponseMediaType]) -> Vec<(String, Vec<ResponseMediaType>)> {
    media_types
      .iter()
      .filter_map(|media_type| {
        let schema = media_type.schema_type.as_ref()?;
        let key = match media_type.category {
          ContentCategory::EventStream => format!("oas3_gen_support::EventStream<{}>", schema.to_rust_type()),
          _ => schema.to_rust_type(),
        };
        Some((key, media_type.clone()))
      })
      .fold(
        IndexMap::<String, Vec<ResponseMediaType>>::new(),
        |mut groups, (key, item)| {
          groups.entry(key).or_default().push(item);
          groups
        },
      )
      .into_iter()
      .collect()
  }
}
