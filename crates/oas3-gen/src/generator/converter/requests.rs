use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Operation},
};

use super::{
  SchemaConverter,
  parameters::{ConvertedParams, ParameterConverter},
  responses,
};
use crate::generator::{
  ast::{
    BuilderField, BuilderNestedStruct, Documentation, EnumToken, FieldDef, FieldNameToken, MethodNameToken,
    ResponseEnumDef, RustType, StructDef, StructKind, StructMethod, StructMethodKind, StructToken, TypeRef,
  },
  naming::{
    constants::{BODY_FIELD_NAME, REQUEST_BODY_SUFFIX},
    identifiers::to_rust_type_name,
    inference::InferenceExt,
  },
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
pub(crate) struct RequestConverter<'a> {
  schema_converter: &'a SchemaConverter,
  spec: &'a Spec,
}

impl<'a> RequestConverter<'a> {
  /// Creates a new request converter.
  pub(crate) fn new(schema_converter: &'a SchemaConverter, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  /// Builds a request struct for an operation.
  ///
  /// Converts parameters, resolves request body, and generates builder methods.
  pub(crate) fn build(
    &self,
    name: &str,
    path: &str,
    operation: &Operation,
    response_enum: Option<(&EnumToken, &ResponseEnumDef)>,
  ) -> anyhow::Result<RequestOutput> {
    let params = ParameterConverter::new(self.schema_converter, self.spec).convert_all(name, path, operation)?;

    let ConvertedParams {
      mut main_fields,
      nested_structs,
      all_fields,
      inline_types,
      warnings,
    } = params;

    if let Some(body_field) = self.create_body_field(operation, path)? {
      main_fields.push(body_field);
    }

    let mut methods = vec![];
    if let Some((enum_token, def)) = response_enum {
      methods.push(responses::build_parse_response_method(enum_token, &def.variants));
    }
    if let Some(builder) = Self::build_builder_method(&nested_structs, &main_fields) {
      methods.push(builder);
    }

    let main_struct = StructDef::builder()
      .name(StructToken::new(name))
      .docs(Documentation::from_optional(
        operation.description.as_ref().or(operation.summary.as_ref()),
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

  fn create_body_field(&self, operation: &Operation, path: &str) -> anyhow::Result<Option<FieldDef>> {
    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok(None);
    };

    let body = body_ref.resolve(self.spec)?;
    let is_required = body.required.unwrap_or(false);

    let Some((_, media_type)) = body.content.iter().next() else {
      return Ok(None);
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(None);
    };

    let body_type = self.resolve_body_type(schema_ref, path)?;
    let Some(type_ref) = body_type else {
      return Ok(None);
    };

    let rust_type = if is_required { type_ref } else { type_ref.with_option() };

    Ok(Some(
      FieldDef::builder()
        .name(FieldNameToken::from_raw(BODY_FIELD_NAME))
        .docs(Documentation::from_optional(body.description.as_ref()))
        .rust_type(rust_type)
        .build(),
    ))
  }

  fn resolve_body_type(
    &self,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    path: &str,
  ) -> anyhow::Result<Option<TypeRef>> {
    match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(name) = SchemaRegistry::parse_ref(ref_path) else {
          return Ok(None);
        };
        Ok(Some(TypeRef::new(to_rust_type_name(&name))))
      }
      ObjectOrReference::Object(schema) => {
        let base_name = schema.infer_name_from_context(path, REQUEST_BODY_SUFFIX);
        let Some(output) = self.schema_converter.convert_inline_schema(schema, &base_name)? else {
          return Ok(None);
        };
        Ok(Some(TypeRef::new(output.type_name)))
      }
    }
  }

  fn build_builder_method(nested_structs: &[StructDef], main_fields: &[FieldDef]) -> Option<StructMethod> {
    let mut builder_fields = vec![];
    let mut nested_info = vec![];

    for main_field in main_fields {
      let nested = nested_structs
        .iter()
        .find(|s| s.name.to_string() == main_field.rust_type.to_rust_type());

      if let Some(nested_struct) = nested {
        nested_info.push(BuilderNestedStruct {
          field_name: main_field.name.clone(),
          struct_name: nested_struct.name.clone(),
          field_names: nested_struct.fields.iter().map(|f| f.name.clone()).collect(),
        });

        for field in &nested_struct.fields {
          builder_fields.push(BuilderField {
            name: field.name.clone(),
            rust_type: if field.is_required() {
              field.rust_type.clone().unwrap_option()
            } else {
              field.rust_type.clone()
            },
            owner_field: Some(main_field.name.clone()),
          });
        }
      } else {
        builder_fields.push(BuilderField {
          name: main_field.name.clone(),
          rust_type: if main_field.is_required() {
            main_field.rust_type.clone().unwrap_option()
          } else {
            main_field.rust_type.clone()
          },
          owner_field: None,
        });
      }
    }

    if builder_fields.is_empty() {
      return None;
    }

    Some(
      StructMethod::builder()
        .name(MethodNameToken::from_raw("new"))
        .docs(Documentation::from_lines([
          "Create a new request with the given parameters.",
        ]))
        .kind(StructMethodKind::Builder {
          fields: builder_fields,
          nested_structs: nested_info,
        })
        .build(),
    )
  }
}

/// Information about a request body for operation metadata.
#[derive(Debug, Clone, Default)]
pub(crate) struct BodyInfo {
  pub(crate) generated_types: Vec<RustType>,
  pub(crate) type_usage: Vec<String>,
  pub(crate) field_name: Option<FieldNameToken>,
  pub(crate) optional: bool,
  pub(crate) content_type: Option<String>,
}

impl BodyInfo {
  pub(crate) fn empty(optional: bool) -> Self {
    Self {
      optional,
      ..Default::default()
    }
  }
}

/// Prepares request body information for an operation.
///
/// This is used by the operation converter to track body metadata
/// separately from request struct generation.
pub(crate) fn prepare_body_info(
  schema_converter: &SchemaConverter,
  spec: &Spec,
  operation: &Operation,
  path: &str,
) -> anyhow::Result<BodyInfo> {
  let Some(body_ref) = operation.request_body.as_ref() else {
    return Ok(BodyInfo::empty(true));
  };

  let body = body_ref.resolve(spec)?;
  let is_required = body.required.unwrap_or(false);

  let Some((content_type, media_type)) = body.content.iter().next() else {
    return Ok(BodyInfo::empty(!is_required));
  };

  let Some(schema_ref) = media_type.schema.as_ref() else {
    return Ok(BodyInfo::empty(!is_required));
  };

  let (generated_types, type_usage) = match schema_ref {
    ObjectOrReference::Ref { ref_path, .. } => {
      let Some(name) = SchemaRegistry::parse_ref(ref_path) else {
        return Ok(BodyInfo::empty(!is_required));
      };
      let rust_name = to_rust_type_name(&name);
      (vec![], vec![rust_name])
    }
    ObjectOrReference::Object(schema) => {
      let base_name = schema.infer_name_from_context(path, REQUEST_BODY_SUFFIX);
      let Some(output) = schema_converter.convert_inline_schema(schema, &base_name)? else {
        return Ok(BodyInfo::empty(!is_required));
      };
      (output.generated_types, vec![output.type_name])
    }
  };

  Ok(BodyInfo {
    generated_types,
    type_usage,
    field_name: Some(FieldNameToken::new(BODY_FIELD_NAME)),
    optional: !is_required,
    content_type: Some(content_type.clone()),
  })
}
