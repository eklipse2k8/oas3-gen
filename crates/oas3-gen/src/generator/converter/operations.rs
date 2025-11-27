use http::Method;
use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn},
};
use serde_json::Value;

use super::{SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache, metadata, path_renderer, responses};
use crate::generator::{
  ast::{
    ContentCategory, DeriveTrait, EnumToken, FieldDef, FieldNameToken, OperationBody, OperationInfo,
    OperationParameter, ParameterLocation, ResponseEnumDef, RustType, StructDef, StructKind, StructToken, TypeAliasDef,
    TypeRef, ValidationAttribute,
  },
  naming::{
    constants::{BODY_FIELD_NAME, REQUEST_BODY_SUFFIX},
    identifiers::{to_rust_field_name, to_rust_type_name},
    inference as naming,
    operations::{generate_unique_request_name, generate_unique_response_name},
    responses as naming_responses,
  },
  schema_registry::SchemaRegistry,
};

type ParameterValidation = (TypeRef, Vec<ValidationAttribute>, Option<Value>);

struct RequestBodyInfo {
  body_type: Option<TypeRef>,
  generated_types: Vec<RustType>,
  type_usage: Vec<String>,
  field_name: Option<FieldNameToken>,
  optional: bool,
  content_type: Option<String>,
}

impl RequestBodyInfo {
  fn empty(optional: bool) -> Self {
    Self {
      body_type: None,
      generated_types: vec![],
      type_usage: vec![],
      field_name: None,
      optional,
      content_type: None,
    }
  }
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
  schema_converter: &'a SchemaConverter,
  spec: &'a Spec,
}

impl<'a> OperationConverter<'a> {
  pub(crate) fn new(schema_converter: &'a SchemaConverter, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
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
  ) -> anyhow::Result<(Vec<RustType>, OperationInfo)> {
    let base_name = to_rust_type_name(operation_id);
    let stable_id = stable_id.to_string();

    let mut warnings = vec![];
    let mut types = vec![];

    let body_info = self.prepare_request_body(&base_name, operation, path, usage, schema_cache)?;
    types.extend(body_info.generated_types);
    usage.mark_request_iter(&body_info.type_usage);

    let mut response_enum_info = if operation.responses.is_some() {
      let response_name = generate_unique_response_name(&base_name, |name| self.schema_converter.is_schema_name(name));
      responses::build_response_enum(
        self.schema_converter,
        self.spec,
        &response_name,
        operation,
        path,
        schema_cache,
      )
      .map(|def| (EnumToken::new(&response_name), def))
    } else {
      None
    };

    let request_name = generate_unique_request_name(&base_name, |name| self.schema_converter.is_schema_name(name));
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

    let mut request_type_name: Option<StructToken> = None;
    if should_generate_request_struct {
      let rust_request_name = request_struct.name.clone();
      usage.mark_request(rust_request_name.clone());
      types.push(RustType::Struct(request_struct));
      request_type_name = Some(rust_request_name);
    }

    if let Some((_, def)) = response_enum_info.as_mut() {
      def.request_type.clone_from(&request_type_name);
    }

    let response_enum = if let Some((enum_token, def)) = response_enum_info {
      usage.mark_response(def.name.clone());
      for variant in &def.variants {
        if let Some(schema_type) = &variant.schema_type {
          usage.mark_response_type_ref(schema_type);
        }
      }
      types.push(RustType::ResponseEnum(def));
      Some(enum_token)
    } else {
      None
    };

    let response_type_name = naming_responses::extract_response_type_name(self.spec, operation);
    let response_content_category = naming_responses::extract_response_content_type(self.spec, operation)
      .as_deref()
      .map_or(ContentCategory::Json, ContentCategory::from_content_type);
    let response_types = naming_responses::extract_all_response_types(self.spec, operation);
    if let Some(name) = &response_type_name {
      usage.mark_response(name);
    }
    usage.mark_response_iter(&response_types.success);
    usage.mark_response_iter(&response_types.error);

    let body_metadata = body_info.field_name.as_ref().map(|field_name| {
      let content_category = body_info
        .content_type
        .as_deref()
        .map_or(ContentCategory::Json, ContentCategory::from_content_type);
      OperationBody {
        field_name: field_name.clone(),
        optional: body_info.optional,
        content_category,
      }
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
      response_enum,
      response_content_category,
      success_response_types: response_types.success,
      error_response_types: response_types.error,
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
    response_enum_info: Option<&(EnumToken, ResponseEnumDef)>,
  ) -> anyhow::Result<(StructDef, Vec<String>, Vec<OperationParameter>)> {
    let mut warnings = vec![];
    let mut fields = vec![];
    let mut param_mappings = ParameterMappings::default();
    let mut parameter_info = vec![];

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

    if let Some((response_enum, response_enum_def)) = response_enum_info {
      methods.push(responses::build_parse_response_method(
        response_enum,
        &response_enum_def.variants,
      ));
    }

    let struct_def = StructDef {
      name: StructToken::from_raw(name),
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
  ) -> anyhow::Result<RequestBodyInfo> {
    let mut generated_types = vec![];
    let mut type_usage = vec![];

    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok(RequestBodyInfo::empty(true));
    };

    let body = body_ref.resolve(self.spec)?;
    let is_required = body.required.unwrap_or(false);

    let Some((_, media_type)) = body.content.iter().next() else {
      return Ok(RequestBodyInfo::empty(!is_required));
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(RequestBodyInfo::empty(!is_required));
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
    let field_name = body_type.as_ref().map(|_| FieldNameToken::new(BODY_FIELD_NAME));

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
  ) -> anyhow::Result<Option<TypeRef>> {
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

          let result = self.schema_converter.convert_struct(
            &unique_name,
            inline_schema,
            Some(StructKind::RequestBody),
            Some(ctx.schema_cache),
          )?;

          ctx
            .schema_cache
            .register_type(inline_schema, &unique_name, result.inline_types, result.result)?
        };

        ctx.type_usage.push(final_type_name.clone());
        ctx.usage.mark_request(&final_type_name);
        Ok(Some(TypeRef::new(final_type_name)))
      }
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(target_name) = SchemaRegistry::extract_ref_name(ref_path) else {
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
    let body_ref = operation.request_body.as_ref()?;
    let body = body_ref.resolve(self.spec).ok()?;
    let is_required = body.required.unwrap_or(false);

    let docs = body
      .description
      .as_ref()
      .map_or_else(Vec::new, |d| metadata::extract_docs(Some(d)));

    Some(FieldDef {
      name: FieldNameToken::new(BODY_FIELD_NAME),
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
    let mut params = vec![];

    if let Some(path_item) = self.spec.paths.as_ref().and_then(|p| p.get(path)) {
      for param_ref in &path_item.parameters {
        if let Ok(param) = param_ref.resolve(self.spec) {
          params.push(param);
        }
      }
    }

    for param_ref in &operation.parameters {
      if let Ok(param) = param_ref.resolve(self.spec) {
        let param_key = (param.location, param.name.clone());
        params.retain(|p| (p.location, p.name.clone()) != param_key);
        params.push(param);
      }
    }

    params
  }

  fn convert_parameter(
    &self,
    param: &Parameter,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<(FieldDef, OperationParameter)> {
    let (rust_type, validation_attrs, default_value) = self.extract_parameter_type_and_validation(param, warnings)?;

    let is_required = param.required.unwrap_or(false);
    let docs = metadata::extract_docs(param.description.as_ref());

    let final_rust_type = if is_required {
      rust_type.clone()
    } else {
      rust_type.clone().with_option()
    };

    let rust_field_str = to_rust_field_name(&param.name);
    let rust_field = FieldNameToken::new(rust_field_str.clone());

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
  ) -> anyhow::Result<ParameterValidation> {
    let Some(schema_ref) = param.schema.as_ref() else {
      warnings.push(format!(
        "Parameter '{}' has no schema, defaulting to String.",
        param.name
      ));
      return Ok((TypeRef::new("String"), vec![], None));
    };

    let schema = schema_ref.resolve(self.spec)?;
    let type_ref = self.schema_converter.schema_to_type_ref(&schema)?;
    let is_required = param.required.unwrap_or(false);
    let mut validation = metadata::extract_validation_attrs(is_required, &schema, &type_ref);
    if let Some(regex_attr) = ValidationAttribute::extract_regex_if_applicable(&param.name, &schema, &type_ref) {
      validation.push(regex_attr);
    }
    let default = metadata::extract_default_value(&schema);

    Ok((type_ref, validation, default))
  }

  fn map_parameter(param: &Parameter, field: &FieldDef, mappings: &mut ParameterMappings) {
    match param.location {
      ParameterIn::Path => mappings.path.push(path_renderer::PathParamMapping {
        rust_field: field.name.to_string(),
        original_name: param.name.clone(),
      }),
      ParameterIn::Query => mappings.query.push(path_renderer::QueryParamMapping {
        rust_field: field.name.to_string(),
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
