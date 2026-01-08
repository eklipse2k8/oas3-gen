use std::rc::Rc;

use oas3::spec::ObjectOrReference;

use super::{
  inline_resolver::InlineTypeResolver,
  methods::MethodGenerator,
  parameters::{ConvertedParams, ParameterConverter},
};
use crate::generator::{
  ast::{Documentation, FieldDef, FieldNameToken, RustType, StructDef, StructKind, StructMethod, StructToken, TypeRef},
  converter::ConverterContext,
  naming::{
    constants::{BODY_FIELD_NAME, REQUEST_BODY_SUFFIX},
    identifiers::to_rust_type_name,
    inference::InferenceExt,
  },
  operation_registry::OperationEntry,
  schema_registry::SchemaRegistry,
};

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
  pub(crate) content_type: Option<String>,
}

impl BodyInfo {
  /// Prepares request body information for an operation.
  ///
  /// This is used by the operation converter to track body metadata
  /// separately from request struct generation.
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
        let Some(name) = SchemaRegistry::parse_ref(ref_path) else {
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

    Ok(Self {
      generated_types,
      type_usage: vec![type_name],
      field_name: Some(FieldNameToken::new(BODY_FIELD_NAME)),
      body_type: Some(body_type),
      description: body.description.clone(),
      optional: !is_required,
      content_type: Some(content_type.clone()),
    })
  }

  pub(crate) fn create_field(&self) -> Option<FieldDef> {
    let type_ref = self.body_type.clone()?;
    let rust_type = if self.optional {
      type_ref.with_option()
    } else {
      type_ref
    };

    Some(
      FieldDef::builder()
        .name(FieldNameToken::from_raw(BODY_FIELD_NAME))
        .docs(Documentation::from_optional(self.description.as_ref()))
        .rust_type(rust_type)
        .build(),
    )
  }

  fn empty(optional: bool) -> Self {
    Self {
      optional,
      ..Default::default()
    }
  }
}
