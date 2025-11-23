use http::Method;
use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn},
};
use serde_json::Value;

use super::{
  ConversionResult, SchemaConverter, TypeUsageRecorder,
  cache::SharedSchemaCache,
  constants::{
    BODY_FIELD_NAME, REQUEST_BODY_SUFFIX, REQUEST_PARAMS_SUFFIX, REQUEST_SUFFIX, RESPONSE_ENUM_SUFFIX, RESPONSE_SUFFIX,
  },
  metadata, path_renderer, responses,
};
use crate::{
  generator::{
    ast::{
      DeriveTrait, FieldDef, OperationBody, OperationInfo, OperationParameter, ParameterLocation, ResponseEnumDef,
      RustType, StructDef, StructKind, TypeAliasDef, TypeRef,
    },
    schema_graph::SchemaGraph,
  },
  naming::{
    identifiers::{to_rust_field_name, to_rust_type_name},
    inference as naming,
  },
};

type ParameterValidation = (TypeRef, Vec<String>, Option<String>, Option<Value>);

struct RequestBodyInfo {
  body_type: Option<TypeRef>,
  generated_types: Vec<RustType>,
  type_usage: Vec<String>,
  field_name: Option<String>,
  optional: bool,
  content_type: Option<String>,
}

#[derive(Default)]
struct ParameterMappings {
  path: Vec<path_renderer::PathParamMapping>,
  query: Vec<path_renderer::QueryParamMapping>,
}

struct ProcessingContext<'a> {
  type_usage: &'a mut Vec<String>,
  generated_types: &'a mut Vec<RustType>,
  usage: &'a mut TypeUsageRecorder,
  schema_cache: &'a mut SharedSchemaCache,
}

/// Converter for OpenAPI Operations into Rust request/response types.
///
/// Handles generation of request parameter structs, request body types,
/// and response enums/structs for each operation.
pub(crate) struct OperationConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> OperationConverter<'a> {
  pub(crate) fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  fn generate_unique_response_name(&self, base_name: &str) -> String {
    let mut response_name = format!("{base_name}{RESPONSE_SUFFIX}");
    let rust_response_name = to_rust_type_name(&response_name);

    if self.schema_converter.is_schema_name(&rust_response_name) {
      response_name = format!("{base_name}{RESPONSE_SUFFIX}{RESPONSE_ENUM_SUFFIX}");
    }

    response_name
  }

  fn generate_unique_request_name(&self, base_name: &str) -> String {
    let mut request_name = format!("{base_name}{REQUEST_SUFFIX}");
    let rust_request_name = to_rust_type_name(&request_name);

    if self.schema_converter.is_schema_name(&rust_request_name) {
      request_name = format!("{base_name}{REQUEST_SUFFIX}{REQUEST_PARAMS_SUFFIX}");
    }

    request_name
  }

  /// Converts an OpenAPI operation into a set of Rust types and metadata.
  ///
  /// Generates request structs, response enums, and body types.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn convert(
    &self,
    stable_id: &str,
    operation_id: &str,
    method: &Method,
    path: &str,
    operation: &Operation,
    usage: &mut TypeUsageRecorder,
    schema_cache: &mut SharedSchemaCache,
  ) -> ConversionResult<(Vec<RustType>, OperationInfo)> {
    let base_name = to_rust_type_name(operation_id);
    let stable_id = stable_id.to_string();

    let mut warnings = Vec::new();
    let mut types = Vec::new();

    let body_info = self.prepare_request_body(&base_name, operation, path, usage, schema_cache)?;
    types.extend(body_info.generated_types);
    usage.mark_request_iter(&body_info.type_usage);

    let mut response_enum_info = if operation.responses.is_some() {
      let response_name = self.generate_unique_response_name(&base_name);
      responses::build_response_enum(
        self.schema_converter,
        self.spec,
        &response_name,
        None,
        operation,
        path,
        schema_cache,
      )
      .map(|def| (response_name, def))
    } else {
      None
    };

    let request_name = self.generate_unique_request_name(&base_name);
    let (request_struct, request_warnings, parameter_metadata) = self.build_request_struct(
      &request_name,
      path,
      operation,
      body_info.body_type.clone(),
      response_enum_info.as_ref(),
    )?;

    warnings.extend(request_warnings);
    let has_fields = !request_struct.fields.is_empty();
    let should_generate_request_struct = has_fields || response_enum_info.is_some();

    let mut request_type_name = None;
    if should_generate_request_struct {
      let rust_request_name = request_struct.name.clone();
      usage.mark_request(&rust_request_name);
      types.push(RustType::Struct(request_struct));
      request_type_name = Some(rust_request_name);
    }

    if let Some((_, def)) = response_enum_info.as_mut() {
      def.request_type = request_type_name.clone().unwrap_or_default();
    }

    let response_enum_name = if let Some((name, def)) = response_enum_info {
      usage.mark_response(&def.name);
      for variant in &def.variants {
        if let Some(schema_type) = &variant.schema_type {
          usage.mark_response_type_ref(schema_type);
        }
      }
      types.push(RustType::ResponseEnum(def));
      Some(name)
    } else {
      None
    };

    let response_type_name = responses::extract_response_type_name(self.spec, operation);
    let response_content_type = responses::extract_response_content_type(self.spec, operation);
    let (success_response_types, error_response_types) = responses::extract_all_response_types(self.spec, operation);
    if let Some(name) = &response_type_name {
      usage.mark_response(name);
    }
    usage.mark_response_iter(&success_response_types);
    usage.mark_response_iter(&error_response_types);

    let body_metadata = body_info.field_name.as_ref().map(|field_name| OperationBody {
      field_name: field_name.clone(),
      optional: body_info.optional,
      content_type: body_info.content_type.clone(),
    });

    let final_operation_id = operation.operation_id.clone().unwrap_or(base_name);

    let op_info = OperationInfo {
      stable_id,
      operation_id: final_operation_id,
      method: method.clone(),
      path: path.to_string(),
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_type_name,
      response_enum: response_enum_name,
      response_content_type,
      request_body_types: body_info.type_usage,
      success_response_types,
      error_response_types,
      warnings,
      parameters: parameter_metadata,
      body: body_metadata,
    };

    Ok((types, op_info))
  }

  fn build_request_struct(
    &self,
    name: &str,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
    response_enum_info: Option<&(String, ResponseEnumDef)>,
  ) -> ConversionResult<(StructDef, Vec<String>, Vec<OperationParameter>)> {
    let mut warnings = Vec::new();
    let mut fields = Vec::new();
    let mut param_mappings = ParameterMappings::default();
    let mut parameter_info = Vec::new();

    for param in self.collect_parameters(path, operation) {
      let (field, meta) = self.convert_parameter(&param, &mut warnings)?;
      Self::map_parameter(&param, &field, &mut param_mappings);
      fields.push(field);
      parameter_info.push(meta);
    }

    if let Some(body_type_ref) = body_type
      && let Some(body_field) = self.create_body_field(operation, body_type_ref)
    {
      fields.push(body_field);
    }

    let docs = operation
      .description
      .as_ref()
      .or(operation.summary.as_ref())
      .map_or_else(Vec::new, |d| metadata::extract_docs(Some(d)));

    let mut methods = vec![path_renderer::build_render_path_method(
      path,
      &param_mappings.path,
      &param_mappings.query,
    )];

    if let Some((response_enum_name, response_enum_def)) = response_enum_info {
      methods.push(responses::build_parse_response_method(
        response_enum_name,
        &response_enum_def.variants,
      ));
    }

    let struct_def = StructDef {
      name: to_rust_type_name(name),
      docs,
      fields,
      derives: [DeriveTrait::Debug, DeriveTrait::Clone, DeriveTrait::Default]
        .into_iter()
        .collect(),
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods,
      kind: StructKind::OperationRequest,
    };

    Ok((struct_def, warnings, parameter_info))
  }

  fn prepare_request_body(
    &self,
    base_name: &str,
    operation: &Operation,
    path: &str,
    usage: &mut TypeUsageRecorder,
    schema_cache: &mut SharedSchemaCache,
  ) -> ConversionResult<RequestBodyInfo> {
    let mut generated_types = Vec::new();
    let mut type_usage = Vec::new();

    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
        field_name: None,
        optional: true,
        content_type: None,
      });
    };

    let body = body_ref.resolve(self.spec)?;
    let is_required = body.required.unwrap_or(false);

    let Some((_content_type, media_type)) = body.content.iter().next() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
        field_name: None,
        optional: !is_required,
        content_type: None,
      });
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
        field_name: None,
        optional: !is_required,
        content_type: None,
      });
    };

    let raw_body_type_name = format!("{base_name}{REQUEST_BODY_SUFFIX}");
    let mut ctx = ProcessingContext {
      type_usage: &mut type_usage,
      generated_types: &mut generated_types,
      usage,
      schema_cache,
    };
    let body_type = self.process_request_body_schema(
      schema_ref,
      &raw_body_type_name,
      path,
      body.description.as_ref(),
      &mut ctx,
    )?;

    let content_type = body.content.keys().next().cloned();
    let field_name = body_type.as_ref().map(|_| BODY_FIELD_NAME.to_string());

    Ok(RequestBodyInfo {
      body_type,
      generated_types,
      type_usage,
      field_name,
      optional: !is_required,
      content_type,
    })
  }

  fn process_request_body_schema(
    &self,
    schema_ref: &ObjectOrReference<oas3::spec::ObjectSchema>,
    type_name: &str,
    path: &str,
    description: Option<&String>,
    ctx: &mut ProcessingContext,
  ) -> ConversionResult<Option<TypeRef>> {
    let rust_type_name = to_rust_type_name(type_name);

    match schema_ref {
      ObjectOrReference::Object(inline_schema) => {
        if inline_schema.properties.is_empty() {
          return Ok(None);
        }

        let cached_type_name = ctx.schema_cache.get_type_name(inline_schema)?;

        let final_type_name = if let Some(name) = cached_type_name {
          name
        } else {
          let base_name = naming::infer_name_from_context(inline_schema, path, "RequestBody");
          let unique_name = ctx.schema_cache.make_unique_name(&base_name);

          let (body_struct, nested_types) = self.schema_converter.convert_struct(
            &unique_name,
            inline_schema,
            Some(StructKind::RequestBody),
            Some(ctx.schema_cache),
          )?;

          ctx
            .schema_cache
            .register_type(inline_schema, &unique_name, nested_types, body_struct)?
        };

        ctx.type_usage.push(final_type_name.clone());
        ctx.usage.mark_request(&final_type_name);
        Ok(Some(TypeRef::new(final_type_name)))
      }
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(target_name) = SchemaGraph::extract_ref_name(ref_path) else {
          return Ok(None);
        };
        let target_rust_name = to_rust_type_name(&target_name);
        let alias = TypeAliasDef {
          name: rust_type_name.clone(),
          docs: metadata::extract_docs(description),
          target: TypeRef::new(target_rust_name.clone()),
        };
        ctx.generated_types.push(RustType::TypeAlias(alias));
        ctx.type_usage.push(target_rust_name.clone());
        ctx.type_usage.push(rust_type_name.clone());
        ctx.usage.mark_request(&target_rust_name);
        ctx.usage.mark_request(&rust_type_name);
        Ok(Some(TypeRef::new(rust_type_name)))
      }
    }
  }

  fn create_body_field(&self, operation: &Operation, body_type: TypeRef) -> Option<FieldDef> {
    #[allow(clippy::question_mark)]
    let Some(body) = operation.request_body.as_ref().and_then(|r| r.resolve(self.spec).ok()) else {
      return None;
    };
    let is_required = body.required.unwrap_or(false);

    let docs = body
      .description
      .as_ref()
      .map_or_else(Vec::new, |d| metadata::extract_docs(Some(d)));

    Some(FieldDef {
      name: BODY_FIELD_NAME.to_string(),
      docs,
      rust_type: if is_required {
        body_type
      } else {
        body_type.with_option()
      },
      ..Default::default()
    })
  }

  fn collect_parameters(&self, path: &str, operation: &Operation) -> Vec<Parameter> {
    let mut params = Vec::new();

    if let Some(path_item) = self.spec.paths.as_ref().and_then(|p| p.get(path)) {
      for param_ref in &path_item.parameters {
        if let Ok(param) = param_ref.resolve(self.spec) {
          params.push(param);
        }
      }
    }

    for param_ref in &operation.parameters {
      if let Ok(param) = param_ref.resolve(self.spec) {
        let key = (param.location, param.name.clone());
        params.retain(|p| (p.location, p.name.clone()) != key);
        params.push(param);
      }
    }

    params
  }

  fn convert_parameter(
    &self,
    param: &Parameter,
    warnings: &mut Vec<String>,
  ) -> ConversionResult<(FieldDef, OperationParameter)> {
    let (rust_type, validation_attrs, regex_validation, default_value) =
      self.extract_parameter_type_and_validation(param, warnings)?;

    let is_required = param.required.unwrap_or(false);
    let docs = metadata::extract_docs(param.description.as_ref());

    let final_rust_type = if is_required {
      rust_type.clone()
    } else {
      rust_type.clone().with_option()
    };

    let rust_field = to_rust_field_name(&param.name);

    let location = match param.location {
      ParameterIn::Path => ParameterLocation::Path,
      ParameterIn::Query => ParameterLocation::Query,
      ParameterIn::Header => ParameterLocation::Header,
      ParameterIn::Cookie => ParameterLocation::Cookie,
    };

    let field = FieldDef {
      name: rust_field.clone(),
      docs,
      rust_type: final_rust_type.clone(),
      validation_attrs,
      regex_validation,
      default_value,
      example_value: param.example.clone(),
      parameter_location: Some(location),
      ..Default::default()
    };

    let metadata = OperationParameter {
      original_name: param.name.clone(),
      rust_field,
      location,
      required: is_required,
      rust_type: final_rust_type,
    };

    Ok((field, metadata))
  }

  fn extract_parameter_type_and_validation(
    &self,
    param: &Parameter,
    warnings: &mut Vec<String>,
  ) -> ConversionResult<ParameterValidation> {
    let Some(schema_ref) = param.schema.as_ref() else {
      warnings.push(format!(
        "Parameter '{}' has no schema, defaulting to String.",
        param.name
      ));
      return Ok((TypeRef::new("String"), vec![], None, None));
    };

    let schema = schema_ref.resolve(self.spec)?;
    let type_ref = self.schema_converter.schema_to_type_ref(&schema)?;
    let is_required = param.required.unwrap_or(false);
    let validation = metadata::extract_validation_attrs(is_required, &schema, &type_ref);
    let regex = metadata::extract_validation_pattern(&param.name, &schema).cloned();
    let default = metadata::extract_default_value(&schema);

    Ok((type_ref, validation, regex, default))
  }

  fn map_parameter(param: &Parameter, field: &FieldDef, mappings: &mut ParameterMappings) {
    match param.location {
      ParameterIn::Path => mappings.path.push(path_renderer::PathParamMapping {
        rust_field: field.name.clone(),
        original_name: param.name.clone(),
      }),
      ParameterIn::Query => mappings.query.push(path_renderer::QueryParamMapping {
        rust_field: field.name.clone(),
        original_name: param.name.clone(),
        explode: path_renderer::query_param_explode(param),
        style: param.style,
        optional: field.rust_type.nullable,
        is_array: field.rust_type.is_array,
      }),
      _ => {}
    }
  }
}
