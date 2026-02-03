use std::rc::Rc;

use oas3::spec::ObjectOrReference;

use super::{
  inline_resolver::InlineTypeResolver,
  methods::MethodGenerator,
  parameters::{ConvertedParams, ParameterConverter},
};
use crate::{
  generator::{
    ast::{
      ContentCategory, Documentation, FieldDef, FieldNameToken, MultipartFieldInfo, OperationBody, RustPrimitive,
      RustType, StructDef, StructKind, StructMethod, StructToken, TypeRef,
    },
    converter::ConverterContext,
    naming::{
      constants::{BODY_FIELD_NAME, REQUEST_BODY_SUFFIX},
      identifiers::to_rust_type_name,
    },
    operation_registry::OperationEntry,
  },
  utils::{SchemaExt, parse_schema_ref_path},
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
}

impl RequestConverter {
  /// Creates a new request converter.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      param_converter: ParameterConverter::new(context),
    }
  }

  /// Builds a request struct for an operation.
  ///
  /// Converts parameters, resolves request body, and generates builder methods.
  pub(crate) fn build(
    &self,
    name: &str,
    entry: &OperationEntry,
    body_info: &BodyInfo,
    extra_method: Option<StructMethod>,
  ) -> anyhow::Result<RequestOutput> {
    let params = self.param_converter.convert_all(name, &entry.path, &entry.operation)?;

    let ConvertedParams {
      mut main_fields,
      nested_structs,
      all_fields,
      inline_types,
      warnings,
    } = params;

    if let Some(body_field) = body_info.create_field() {
      main_fields.push(body_field);
    }

    let methods = extra_method
      .into_iter()
      .chain(MethodGenerator::build_builder_method(&nested_structs, &main_fields))
      .collect::<Vec<_>>();

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
  pub(crate) multipart_fields: Option<Vec<MultipartFieldInfo>>,
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
    let (generated_types, type_name) = match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(name) = parse_schema_ref_path(ref_path) else {
          return Ok(Self::empty(!is_required));
        };
        (vec![], to_rust_type_name(&name))
      }
      ObjectOrReference::Object(schema) => {
        let base_name = schema.infer_name_from_context(&entry.path, REQUEST_BODY_SUFFIX);
        let Some(output) = inline_resolver.try_inline_schema(schema, &base_name)? else {
          return Ok(Self::empty(!is_required));
        };
        (output.inline_types, output.result)
      }
    };

    let body_type = TypeRef::new(&type_name);
    let content_category = ContentCategory::from_content_type(content_type);
    let multipart_fields = Self::resolve_multipart_fields(content_category, &body_type, &generated_types);

    Ok(Self {
      generated_types,
      type_usage: vec![type_name],
      field_name: Some(FieldNameToken::new(BODY_FIELD_NAME)),
      body_type: Some(body_type),
      description: body.description.clone(),
      optional: !is_required,
      content_category,
      multipart_fields,
    })
  }

  /// Extracts field information for multipart form data bodies.
  ///
  /// Returns `None` for non-multipart content types. For multipart bodies,
  /// inspects the body struct to determine which fields are binary, nullable,
  /// or require JSON serialization.
  fn resolve_multipart_fields(
    category: ContentCategory,
    body_type: &TypeRef,
    generated_types: &[RustType],
  ) -> Option<Vec<MultipartFieldInfo>> {
    if category != ContentCategory::Multipart {
      return None;
    }

    let body_type_name = body_type.unboxed_base_type_name();

    let struct_def = generated_types.iter().find_map(|t| {
      if let RustType::Struct(def) = t
        && def.name.as_str() == body_type_name
      {
        return Some(def);
      }
      None
    })?;

    let fields = struct_def
      .fields
      .iter()
      .map(|f| MultipartFieldInfo {
        name: f.name.clone(),
        nullable: f.rust_type.nullable,
        is_bytes: matches!(f.rust_type.base_type, RustPrimitive::Bytes),
        requires_json: f.rust_type.requires_json_serialization(),
      })
      .collect();

    Some(fields)
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
        .maybe_multipart_fields(self.multipart_fields.clone())
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
