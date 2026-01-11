use std::{collections::HashSet, rc::Rc};

use indexmap::IndexMap;
use oas3::spec::{MediaType, ObjectOrReference, ObjectSchema, Operation, Response};

use super::{ConverterContext, TypeResolver, TypeUsageRecorder, inline_resolver::InlineTypeResolver};
use crate::{
  generator::{
    ast::{
      ContentCategory, Documentation, EnumToken, EnumVariantToken, MethodKind, MethodNameToken, ResponseEnumDef,
      ResponseMediaType, ResponseStatusCategory, ResponseVariant, ResponseVariantCategory, RustPrimitive,
      StatusCodeToken, StatusHandler, StructMethod, TypeRef,
    },
    converter::GenerationTarget,
    naming::{
      constants::{DEFAULT_MEDIA_TYPE, DEFAULT_RESPONSE_DESCRIPTION, DEFAULT_RESPONSE_VARIANT},
      identifiers::to_rust_type_name,
      responses as naming_responses,
    },
    schema_registry::SchemaRegistry,
  },
  utils::SchemaExt as _,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct ResponseMetadata {
  pub(crate) type_name: Option<String>,
  pub(crate) media_types: Vec<ResponseMediaType>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResponseMetadataOutput {
  pub(crate) metadata: ResponseMetadata,
  pub(crate) usage: TypeUsageRecorder,
}

/// Converts OpenAPI responses into Rust enum definitions.
///
/// Handles status codes, media types, and schema resolution for each response.
#[derive(Debug, Clone)]
pub(crate) struct ResponseConverter {
  type_resolver: TypeResolver,
  inline_resolver: InlineTypeResolver,
  context: Rc<ConverterContext>,
}

impl ResponseConverter {
  /// Creates a new response converter.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let type_resolver = TypeResolver::new(context.clone());
    let inline_resolver = InlineTypeResolver::new(context.clone());
    Self {
      type_resolver,
      inline_resolver,
      context,
    }
  }

  /// Builds a response enum for an operation.
  ///
  /// Returns `None` if the operation has no responses or only empty responses.
  pub(crate) fn build_enum(&self, name: &str, operation: &Operation, path: &str) -> Option<ResponseEnumDef> {
    let spec = self.context.graph().spec();
    let responses = operation.responses.as_ref()?;
    let base_name = to_rust_type_name(name);

    let variants = responses
      .iter()
      .filter_map(|(status_str, resp_ref)| {
        let response = resp_ref.resolve(spec).ok()?;
        let status_code: StatusCodeToken = status_str.parse().unwrap_or(StatusCodeToken::Default);
        let media_types = Self::with_default_media_type(
          self
            .extract_media_types(&response, path, status_code)
            .unwrap_or_default(),
        );

        Some(Self::split_variants_by_content_type(
          status_code,
          &status_code.to_variant_token(),
          response.description.as_ref(),
          &media_types,
        ))
      })
      .flatten()
      .collect::<Vec<_>>();

    let variants = Self::with_default_variant(variants);

    if variants.is_empty() {
      return None;
    }

    Some(
      ResponseEnumDef::builder()
        .name(EnumToken::new(&base_name))
        .docs(Documentation::from_lines([format!(
          "Response types for {}",
          operation.operation_id.as_deref().unwrap_or(&base_name)
        )]))
        .variants(variants)
        .build(),
    )
  }

  pub(crate) fn build_parse_method(&self, response_enum: &EnumToken, variants: &[ResponseVariant]) -> StructMethod {
    let (status_handlers, default_handler) = Self::build_status_handlers(variants);

    // We could combine these into one variant, but we shouldn't generate both server and client code
    // in the same generation.
    match self.context.config.target {
      GenerationTarget::Client => StructMethod::builder()
        .name(MethodNameToken::from_raw("parse_response"))
        .docs(Documentation::from_lines([
          "Parse the HTTP response into the response enum.",
        ]))
        .kind(MethodKind::ParseResponse {
          response_enum: response_enum.clone(),
          status_handlers,
          default_handler,
        })
        .build(),
      GenerationTarget::Server => StructMethod::builder()
        .name(MethodNameToken::from_raw("parse_response"))
        .docs(Documentation::from_lines([
          "Server code does not need to parse responses.",
        ]))
        .kind(MethodKind::IntoAxumResponse {
          response_enum: response_enum.clone(),
          status_handlers,
          default_handler,
        })
        .build(),
    }
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

    let mut usage = TypeUsageRecorder::new();
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
      ObjectOrReference::Object(schema) => self.resolve_inline_schema(schema, path, status_code),
    }
  }

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

  fn with_default_media_type(media_types: Vec<ResponseMediaType>) -> Vec<ResponseMediaType> {
    if media_types.is_empty() {
      vec![ResponseMediaType::new(DEFAULT_MEDIA_TYPE)]
    } else {
      media_types
    }
  }

  fn with_default_variant(variants: Vec<ResponseVariant>) -> Vec<ResponseVariant> {
    if variants.is_empty() || variants.iter().any(|v| v.status_code.is_default()) {
      return variants;
    }

    variants
      .into_iter()
      .chain(std::iter::once(
        ResponseVariant::builder()
          .variant_name(EnumVariantToken::from_raw(DEFAULT_RESPONSE_VARIANT))
          .description(DEFAULT_RESPONSE_DESCRIPTION.to_string())
          .media_types(vec![ResponseMediaType::new(DEFAULT_MEDIA_TYPE)])
          .build(),
      ))
      .collect()
  }

  fn split_variants_by_content_type(
    status_code: StatusCodeToken,
    base_name: &EnumVariantToken,
    description: Option<&String>,
    media_types: &[ResponseMediaType],
  ) -> Vec<ResponseVariant> {
    let grouped = Self::group_media_types_by_schema(media_types);

    if grouped.is_empty() {
      return vec![
        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(base_name.clone())
          .maybe_description(description.cloned())
          .media_types(media_types.to_vec())
          .build(),
      ];
    }

    let needs_suffix = grouped.len() > 1;

    grouped
      .into_iter()
      .map(|(schema_key, types)| {
        let primary_category = types.first().map_or(ContentCategory::Json, |m| m.category);

        ResponseVariant::builder()
          .status_code(status_code)
          .variant_name(if needs_suffix {
            base_name.clone().with_content_suffix(primary_category)
          } else {
            base_name.clone()
          })
          .maybe_description(description.cloned())
          .media_types(types)
          .maybe_schema_type(Some(TypeRef::new(schema_key)))
          .build()
      })
      .collect()
  }

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

  fn build_status_handlers(variants: &[ResponseVariant]) -> (Vec<StatusHandler>, Option<ResponseVariantCategory>) {
    let (default_variants, status_variants): (Vec<_>, Vec<_>) =
      variants.iter().partition(|v| v.status_code.is_default());

    let status_handlers = status_variants
      .into_iter()
      .fold(
        IndexMap::<StatusCodeToken, Vec<&ResponseVariant>>::new(),
        |mut acc, v| {
          acc.entry(v.status_code).or_default().push(v);
          acc
        },
      )
      .into_iter()
      .map(|(code, group)| StatusHandler {
        status_code: code,
        dispatch: ResponseStatusCategory::from_variants(&group),
      })
      .collect();

    let default_handler = default_variants.first().map(|v| ResponseVariantCategory {
      category: ResponseMediaType::primary_category(&v.media_types),
      variant: (*v).clone(),
    });

    (status_handlers, default_handler)
  }
}

impl ResponseStatusCategory {
  #[must_use]
  pub fn from_variants(variants: &[&ResponseVariant]) -> Self {
    if let [variant] = variants {
      let unique_categories = variant
        .media_types
        .iter()
        .map(|m| m.category)
        .collect::<HashSet<_>>()
        .len();

      if unique_categories <= 1 {
        return Self::Single(
          ResponseVariantCategory::builder()
            .category(ResponseMediaType::primary_category(&variant.media_types))
            .variant((*variant).clone())
            .build(),
        );
      }
    }

    Self::from_content_types(variants)
  }

  #[must_use]
  pub(crate) fn from_content_types(variants: &[&ResponseVariant]) -> Self {
    let all_categories = variants
      .iter()
      .flat_map(|variant| {
        let default_category = variant
          .media_types
          .is_empty()
          .then(|| ResponseMediaType::primary_category(&[]));

        let explicit_categories = variant.media_types.iter().map(|m| m.category);

        default_category
          .into_iter()
          .chain(explicit_categories)
          .map(move |category| (category, variant.variant_name.as_str(), (*variant).clone()))
      })
      .fold(
        (HashSet::new(), vec![]),
        |(mut seen, mut result), (category, name, variant)| {
          if seen.insert((category, name)) {
            result.push(ResponseVariantCategory { category, variant });
          }
          (seen, result)
        },
      )
      .1;

    let (streams, variants): (Vec<_>, Vec<_>) = all_categories
      .into_iter()
      .partition(|c| c.category == ContentCategory::EventStream);

    Self::ContentDispatch { streams, variants }
  }
}
