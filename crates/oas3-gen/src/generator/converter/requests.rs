use std::{borrow::Cow, rc::Rc};

use indexmap::IndexMap;
use mediatype::MediaType;
use oas3::{
  Spec,
  spec::{Encoding, MediaType as MediaTypeObject, ObjectSchema, Schema},
};

use super::{
  inline_resolver::InlineTypeResolver,
  methods::MethodGenerator,
  parameters::{ConvertedParams, ParameterConverter},
};
use crate::{
  generator::{
    ast::{
      ContentCategory, Documentation, FieldDef, FieldNameToken, MultipartProperty, OperationBody, PartRfc6570,
      PartStyle, RustPrimitive, RustType, StructDef, StructKind, StructToken, TypeRef, is_json_media_type,
    },
    converter::ConverterContext,
    naming::{
      constants::{BODY_FIELD_NAME, REQUEST_BODY_SUFFIX},
      identifiers::to_rust_type_name,
      operations::credentials_field_name,
    },
    operation_registry::OperationEntry,
  },
  utils::{SchemaExt, SchemaInspect, SchemaRefName, SchemaResolveExt, parse_schema_ref_path},
};

/// Result of building a request struct for an operation.
///
/// Contains the main request struct, nested parameter structs (path, query, header),
/// inline types generated from parameter or body schemas, and any warnings.
#[derive(Debug, Clone)]
pub(crate) struct RequestOutput {
  pub(crate) main_struct: StructDef,
  pub(crate) nested_structs: Vec<StructDef>,
  pub(crate) inline_types: Vec<RustType>,
  pub(crate) parameter_fields: Vec<FieldDef>,
  pub(crate) warnings: Vec<String>,
}

/// Builds request structs from parameters and request bodies.
///
/// Coordinates parameter conversion and body resolution to produce
/// a complete request struct with nested parameter structs.
#[derive(Debug, Clone)]
pub(crate) struct RequestConverter {
  param_converter: ParameterConverter,
  enable_builders: bool,
}

impl RequestConverter {
  /// Creates a new request converter.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      param_converter: ParameterConverter::new(context),
      enable_builders: context.config().enable_builders(),
    }
  }

  /// Builds a request struct for an operation.
  ///
  /// Converts parameters, adds the server's `credentials` field when the operation
  /// accepts API keys, resolves the request body, and generates builder methods.
  pub(crate) fn build(
    &self,
    name: &str,
    entry: &OperationEntry,
    body_info: &BodyInfo,
    credentials_type: Option<&StructToken>,
  ) -> anyhow::Result<RequestOutput> {
    let params = self.param_converter.convert_all(name, &entry.path, &entry.operation)?;

    let ConvertedParams {
      mut main_fields,
      nested_structs,
      all_fields,
      inline_types,
      warnings,
    } = params;

    if let Some(credentials_type) = credentials_type {
      let parameters = all_fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>();
      main_fields.push(FieldDef::nested_struct_field(
        &credentials_field_name(&parameters),
        credentials_type.as_str(),
      ));
    }

    if let Some(body_field) = body_info.create_field() {
      main_fields.push(body_field);
    }

    let builder_method = if self.enable_builders {
      MethodGenerator::build_builder_method(&nested_structs, &main_fields)
    } else {
      None
    };

    let methods = builder_method.into_iter().collect::<Vec<_>>();

    let main_struct = StructDef::builder()
      .name(StructToken::new(name))
      .docs(Documentation::from_optional(
        entry
          .operation
          .description
          .as_ref()
          .or(entry.operation.summary.as_ref()),
      ))
      .fields(main_fields)
      .methods(methods)
      .kind(StructKind::OperationRequest)
      .build();

    Ok(RequestOutput {
      main_struct,
      nested_structs,
      inline_types,
      parameter_fields: all_fields,
      warnings,
    })
  }
}

/// Information about a request body for operation metadata.
#[derive(Debug, Clone, Default)]
pub(crate) struct BodyInfo {
  pub(crate) generated_types: Vec<RustType>,
  pub(crate) type_usage: Vec<String>,
  pub(crate) field_name: Option<FieldNameToken>,
  pub(crate) body_type: Option<TypeRef>,
  pub(crate) description: Option<String>,
  pub(crate) optional: bool,
  pub(crate) content_category: ContentCategory,
  pub(crate) multipart_properties: IndexMap<String, MultipartProperty>,
  pub(crate) warnings: Vec<String>,
}

impl BodyInfo {
  /// Extracts request body information from an operation entry.
  ///
  /// Resolves the body schema (via `$ref` or inline), determines the content
  /// category (JSON, form, multipart, binary), and collects any inline types
  /// generated during schema resolution. Returns an empty body info if no
  /// request body is defined.
  pub(crate) fn new(context: &Rc<ConverterContext>, entry: &OperationEntry) -> anyhow::Result<Self> {
    let spec = context.graph().spec();
    let Some(body_ref) = entry.operation.request_body.as_ref() else {
      return Ok(Self::empty(true));
    };

    let body = body_ref.resolve(spec)?;
    let is_required = body.required.unwrap_or(false);

    let Some((content_type, media_type)) = body.content.iter().next() else {
      return Ok(Self::empty(!is_required));
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(Self::empty(!is_required));
    };

    let inline_resolver = InlineTypeResolver::new(context.clone());
    let (generated_types, type_name) = if matches!(schema_ref, Schema::Boolean(_)) {
      (vec![], RustPrimitive::Value.to_string())
    } else if let Some(ref_path) = schema_ref.ref_path() {
      let Some(name) = parse_schema_ref_path(ref_path) else {
        return Ok(Self::empty(!is_required));
      };
      (vec![], to_rust_type_name(&name))
    } else if let Some(schema) = schema_ref.as_inline() {
      let base_name = schema.infer_name_from_context(&entry.path, REQUEST_BODY_SUFFIX);
      let Some(output) = inline_resolver.try_inline_schema(schema, &base_name)? else {
        return Ok(Self::empty(!is_required));
      };
      (output.inline_types, output.result)
    } else {
      return Ok(Self::empty(!is_required));
    };

    let body_type = TypeRef::new(&type_name);
    let content_category = ContentCategory::from_content_type(content_type);
    let (multipart_properties, warnings) = if content_category == ContentCategory::Multipart {
      let graph = context.graph();
      let schema = match schema_ref.schema_ref_name() {
        Some(name) => graph.resolved(&name).map_or_else(Cow::default, Cow::Borrowed),
        None => Cow::Owned(graph.merge_inline(&schema_ref.resolve_object(spec)?)?),
      };
      MultipartAnalyzer { spec, media_type }.analyze(content_type, &schema)?
    } else {
      (IndexMap::new(), vec![])
    };

    Ok(Self {
      generated_types,
      type_usage: vec![type_name],
      field_name: Some(FieldNameToken::new(BODY_FIELD_NAME)),
      body_type: Some(body_type),
      description: body.description.clone(),
      optional: !is_required,
      content_category,
      multipart_properties,
      warnings,
    })
  }

  /// Creates a field definition for the request body if present.
  pub(crate) fn create_field(&self) -> Option<FieldDef> {
    let type_ref = self.body_type.clone()?;
    Some(FieldDef::body_field(
      BODY_FIELD_NAME,
      self.description.as_ref(),
      type_ref,
      self.optional,
    ))
  }

  /// Converts body info into operation body metadata for code generation.
  pub(crate) fn to_operation_body(&self) -> Option<OperationBody> {
    let field_name = self.field_name.as_ref()?;

    Some(
      OperationBody::builder()
        .field_name(field_name.clone())
        .maybe_body_type(self.body_type.clone())
        .optional(self.optional)
        .content_category(self.content_category)
        .multipart_properties(self.multipart_properties.clone())
        .build(),
    )
  }

  /// Creates an empty body info with the specified optionality.
  fn empty(optional: bool) -> Self {
    Self {
      optional,
      ..Default::default()
    }
  }
}

/// What the spec declares about each multipart property, and warnings for what generated code
/// can't send as written.
type MultipartAnalysis = (IndexMap<String, MultipartProperty>, Vec<String>);

/// Reads what a `multipart/form-data` media type declares about each body property.
struct MultipartAnalyzer<'a> {
  spec: &'a Spec,
  media_type: &'a MediaTypeObject,
}

impl MultipartAnalyzer<'_> {
  /// Collects each property's Encoding Object `contentType` and RFC 6570 fields and whether its
  /// schema is raw binary, leaving out properties with nothing beyond the defaults, along with
  /// warnings for what generated code can't honor.
  fn analyze(&self, content_type: &str, schema: &ObjectSchema) -> anyhow::Result<MultipartAnalysis> {
    let mut properties = IndexMap::new();
    let mut warnings = vec![];
    if MediaType::parse(content_type).is_ok_and(|media| media.subty.as_str() != "form-data") {
      warnings.push(format!(
        "`{content_type}` request bodies are sent as `multipart/form-data`"
      ));
    }
    if schema.properties.is_empty() && schema.additional_properties.is_none() {
      warnings.push(
        "multipart request body is not an object with properties, so its parts come from its JSON form and \
         binary values aren't sent as files"
          .to_string(),
      );
    }

    for (name, property_ref) in &schema.properties {
      let resolved = self.resolve(property_ref)?;
      let encoding = self.media_type.encoding.get(name);
      if name.contains(['"', '\r', '\n']) {
        warnings.push(format!(
          "multipart part name `{name}` contains a quote or line break, which a part header can't carry"
        ));
      }
      if resolved.items.as_ref().is_some_and(SchemaExt::is_array) {
        warnings.push(format!(
          "multipart property `{name}` holds nested arrays, whose inner arrays are sent as JSON text"
        ));
      }
      if let Some(encoding) = encoding {
        warnings.extend(self.encoding_warnings(name, encoding, &resolved));
      }

      let property = MultipartProperty {
        content_type: encoding
          .and_then(|encoding| encoding.content_type.as_deref())
          .and_then(first_concrete_media_type)
          .map(|content_type| {
            if resolved.is_binary() {
              content_type
            } else {
              with_utf8_charset(content_type)
            }
          }),
        rfc6570: encoding.and_then(part_rfc6570),
        raw_binary: resolved.is_raw_binary(),
      };
      if property != MultipartProperty::default() {
        properties.insert(name.clone(), property);
      }
    }

    for name in self.media_type.encoding.keys() {
      if !schema.properties.contains_key(name) {
        warnings.push(format!(
          "multipart encoding names `{name}`, which is not a property of the request body"
        ));
      }
    }
    Ok((properties, warnings))
  }

  fn encoding_warnings(&self, name: &str, encoding: &Encoding, property: &ResolvedProperty) -> Vec<String> {
    let mut warnings = vec![];
    if let Some(declared) = encoding.content_type.as_deref() {
      match first_concrete_media_type(declared) {
        None => warnings.push(format!(
          "multipart property `{name}` declares contentType `{declared}`, which names no media type a part can \
           carry, so its parts use the default"
        )),
        Some(chosen) if property.is_binary() && chosen != declared.trim() => warnings.push(format!(
          "multipart property `{name}` allows `{declared}`, and every file part is labelled `{chosen}`"
        )),
        Some(chosen) if property.is_structured() && !is_json_media_type(&chosen) => warnings.push(format!(
          "multipart property `{name}` declares contentType `{chosen}`, but structured values are sent as \
           `application/json`"
        )),
        Some(chosen) if !property.is_binary() && has_non_utf8_charset(&chosen) => warnings.push(format!(
          "multipart property `{name}` declares contentType `{chosen}`, but text is sent as UTF-8"
        )),
        Some(_) => {}
      }
    }
    if let Some(style) = &encoding.style
      && style.parse::<PartStyle>().is_err()
    {
      warnings.push(format!(
        "multipart property `{name}` declares style `{style}`, which multipart/form-data doesn't support, so \
         `form` is used"
      ));
    }
    for (header, header_ref) in &encoding.headers {
      if !header.eq_ignore_ascii_case("content-type")
        && header_ref
          .resolve(self.spec)
          .is_ok_and(|header| header.required.unwrap_or(false))
      {
        warnings.push(format!(
          "multipart part `{name}` requires header `{header}`, which generated code does not send"
        ));
      }
    }
    warnings
  }

  fn resolve(&self, property_ref: &Schema) -> anyhow::Result<ResolvedProperty> {
    let schema = property_ref.resolve_object(self.spec)?;
    let schema = match schema.single_non_null_variant(self.spec) {
      Some(variant) if schema.has_null_variant(self.spec) => variant.resolve_object(self.spec)?,
      _ => schema,
    };
    let items = schema
      .items
      .as_deref()
      .map(|items| items.resolve_object(self.spec))
      .transpose()?;
    Ok(ResolvedProperty { schema, items })
  }
}

/// A body property's schema, unwrapped from a nullable union, and its `items` schema.
struct ResolvedProperty {
  schema: ObjectSchema,
  items: Option<ObjectSchema>,
}

impl ResolvedProperty {
  /// Whether OpenAPI 3.1 reads the property, or each of its items, as raw binary.
  fn is_raw_binary(&self) -> bool {
    is_raw_binary(&self.schema) || (self.schema.is_array() && self.items.as_ref().is_none_or(is_raw_binary))
  }

  /// Whether the generator sends the property, or each of its items, as file parts.
  fn is_binary(&self) -> bool {
    is_binary(&self.schema) || self.items.as_ref().is_some_and(is_binary)
  }

  /// Whether the property, or each of its items, is an object, which multipart sends as JSON.
  fn is_structured(&self) -> bool {
    is_structured(&self.schema) || self.items.as_ref().is_some_and(is_structured)
  }
}

/// Returns `true` when a schema declares no `type` and constrains nothing about its JSON shape,
/// which OpenAPI 3.1 reads as raw binary (`application/octet-stream`) in `multipart` content.
fn is_raw_binary(schema: &ObjectSchema) -> bool {
  schema.is_empty_object()
    && schema.const_value.is_none()
    && schema.additional_properties.is_none()
    && schema.items.is_none()
    && schema.prefix_items.is_empty()
}

/// Returns `true` for schemas the generator sends as file parts.
fn is_binary(schema: &ObjectSchema) -> bool {
  is_raw_binary(schema) || matches!(schema.format.as_deref(), Some("binary"))
}

/// Returns `true` for object schemas.
fn is_structured(schema: &ObjectSchema) -> bool {
  schema.is_object() || !schema.properties.is_empty() || schema.additional_properties.is_some()
}

/// The first media type an Encoding Object's `contentType` lists that a part can carry: a
/// wildcard such as `image/*` names a range, not a type a sender can label with.
fn first_concrete_media_type(content_type: &str) -> Option<String> {
  content_type
    .split(',')
    .map(str::trim)
    .find(|candidate| {
      MediaType::parse(candidate).is_ok_and(|media| media.ty.as_str() != "*" && media.subty.as_str() != "*")
    })
    .map(str::to_owned)
}

/// Returns `true` when a media type names a `charset` other than UTF-8.
fn has_non_utf8_charset(content_type: &str) -> bool {
  MediaType::parse(content_type).is_ok_and(|media| {
    media.params.iter().any(|(name, value)| {
      name.as_str().eq_ignore_ascii_case("charset") && !value.as_str().eq_ignore_ascii_case("utf-8")
    })
  })
}

/// Replaces a `charset` other than UTF-8, since generated code sends Rust strings as UTF-8.
fn with_utf8_charset(content_type: String) -> String {
  if !has_non_utf8_charset(&content_type) {
    return content_type;
  }
  let Ok(mut media) = MediaType::parse(&content_type) else {
    return content_type;
  };
  media.params = media
    .params
    .iter()
    .filter(|(name, _)| !name.as_str().eq_ignore_ascii_case("charset"))
    .copied()
    .collect::<Vec<_>>()
    .into();
  format!("{media}; charset=utf-8")
}

/// Returns the RFC 6570 serialization an Encoding Object selects. Any explicit `style`,
/// `explode`, or `allowReserved` selects it; `style` defaults to `form`, and `explode` defaults
/// to `true` only for `form`.
fn part_rfc6570(encoding: &Encoding) -> Option<PartRfc6570> {
  if encoding.style.is_none() && encoding.explode.is_none() && encoding.allow_reserved.is_none() {
    return None;
  }
  let style = encoding
    .style
    .as_deref()
    .and_then(|style| style.parse().ok())
    .unwrap_or(PartStyle::Form);
  let explode = encoding.explode.unwrap_or(style == PartStyle::Form);
  Some(PartRfc6570 { style, explode })
}
