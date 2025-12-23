use std::collections::HashSet;

use http::Method;
use oas3::{
  Spec,
  spec::{
    ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, ParameterStyle, SchemaType, SchemaTypeSet,
  },
};
use serde_json::Value;

use super::{SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache, metadata, path_renderer, responses};
use crate::generator::{
  ast::{
    ContentCategory, EnumToken, FieldDef, FieldNameToken, OperationBody, OperationInfo, OperationKind,
    OperationParameter, OuterAttr, ParameterLocation, ParsedPath, ResponseEnumDef, RustType, SerdeAsFieldAttr,
    SerdeAsSeparator, SerdeAttribute, StructDef, StructKind, StructToken, TypeRef, ValidationAttribute,
  },
  naming::{
    constants::{
      BODY_FIELD_NAME, HEADER_PARAMS_FIELD, HEADER_PARAMS_SUFFIX, PATH_PARAMS_FIELD, PATH_PARAMS_SUFFIX,
      QUERY_PARAMS_FIELD, QUERY_PARAMS_SUFFIX, REQUEST_BODY_SUFFIX,
    },
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

struct ParametersByLocation {
  path: Vec<(FieldDef, OperationParameter, ParameterMeta)>,
  query: Vec<(FieldDef, OperationParameter, ParameterMeta)>,
  header: Vec<(FieldDef, OperationParameter, ParameterMeta)>,
}

struct ParameterMeta {
  original_name: String,
  explode: bool,
  style: Option<ParameterStyle>,
}

impl ParametersByLocation {
  fn new() -> Self {
    Self {
      path: vec![],
      query: vec![],
      header: vec![],
    }
  }

  fn has_path_params(&self) -> bool {
    !self.path.is_empty()
  }

  fn has_query_params(&self) -> bool {
    !self.query.is_empty()
  }

  fn has_header_params(&self) -> bool {
    !self.header.is_empty()
  }
}

struct GeneratedRequestStructs {
  main_struct: StructDef,
  nested_structs: Vec<StructDef>,
  parameter_info: Vec<OperationParameter>,
  warnings: Vec<String>,
}

struct RequestBodyOutput {
  body_type: TypeRef,
  generated_types: Vec<RustType>,
  type_usage: Vec<String>,
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
    kind: OperationKind,
    operation: &Operation,
    usage: &mut TypeUsageRecorder,
    schema_cache: &mut SharedSchemaCache,
  ) -> anyhow::Result<(Vec<RustType>, OperationInfo)> {
    let base_name = to_rust_type_name(operation_id);
    let stable_id = stable_id.to_string();

    let mut warnings = vec![];
    let mut types = vec![];

    let body_info = self.prepare_request_body(operation, path, usage, schema_cache)?;
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
    let generated_structs = self.build_request_struct(
      &request_name,
      path,
      operation,
      body_info.body_type.clone(),
      response_enum_info.as_ref(),
    )?;

    warnings.extend(generated_structs.warnings);
    let parameter_metadata = generated_structs.parameter_info;

    let has_fields = !generated_structs.main_struct.fields.is_empty();
    let should_generate_request_struct = has_fields || response_enum_info.is_some();

    let mut request_type_name: Option<StructToken> = None;
    if should_generate_request_struct {
      for nested_struct in generated_structs.nested_structs {
        types.push(RustType::Struct(nested_struct));
      }

      let rust_request_name = generated_structs.main_struct.name.clone();
      usage.mark_request(rust_request_name.clone());
      types.push(RustType::Struct(generated_structs.main_struct));
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
    let parsed_path = ParsedPath::new(path, &parameter_metadata);

    let op_info = OperationInfo {
      stable_id,
      operation_id: final_operation_id,
      method: method.clone(),
      path: parsed_path,
      path_template: path.to_string(),
      kind,
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
  ) -> anyhow::Result<GeneratedRequestStructs> {
    let mut warnings = vec![];
    let mut params_by_location = ParametersByLocation::new();
    let mut all_parameter_info = vec![];

    for param in self.collect_parameters(path, operation) {
      let (field, meta, param_meta) = self.convert_parameter_with_meta(&param, &mut warnings)?;
      all_parameter_info.push(meta.clone());

      match param.location {
        ParameterIn::Path => params_by_location.path.push((field, meta, param_meta)),
        ParameterIn::Query => params_by_location.query.push((field, meta, param_meta)),
        ParameterIn::Header => params_by_location.header.push((field, meta, param_meta)),
        ParameterIn::Cookie => {}
      }
    }

    let mut nested_structs = vec![];
    let mut main_fields = vec![];

    let path_struct = if params_by_location.has_path_params() {
      let struct_name = format!("{name}{PATH_PARAMS_SUFFIX}");
      let fields: Vec<FieldDef> = params_by_location.path.iter().map(|(f, _, _)| f.clone()).collect();
      let struct_def = StructDef {
        name: StructToken::from_raw(&struct_name),
        docs: vec![],
        fields,
        kind: StructKind::PathParams,
        ..Default::default()
      };
      Some(struct_def)
    } else {
      None
    };

    if let Some(ref path_struct) = path_struct {
      main_fields.push(FieldDef {
        name: FieldNameToken::new(PATH_PARAMS_FIELD),
        rust_type: TypeRef::new(path_struct.name.to_string()),
        ..Default::default()
      });
      nested_structs.push(path_struct.clone());
    }

    let query_struct = if params_by_location.has_query_params() {
      let struct_name = format!("{name}{QUERY_PARAMS_SUFFIX}");
      let fields: Vec<FieldDef> = params_by_location
        .query
        .iter()
        .map(|(f, _, meta)| Self::apply_query_serde_attributes(f.clone(), meta))
        .collect();

      let has_serde_as = fields.iter().any(|f| f.serde_as_attr.is_some());

      let outer_attrs = if has_serde_as { vec![OuterAttr::SerdeAs] } else { vec![] };

      let struct_def = StructDef {
        name: StructToken::from_raw(&struct_name),
        docs: vec![],
        fields,
        outer_attrs,
        kind: StructKind::QueryParams,
        ..Default::default()
      };
      Some(struct_def)
    } else {
      None
    };

    if let Some(ref query_struct) = query_struct {
      main_fields.push(FieldDef {
        name: FieldNameToken::new(QUERY_PARAMS_FIELD),
        rust_type: TypeRef::new(query_struct.name.to_string()),
        ..Default::default()
      });
      nested_structs.push(query_struct.clone());
    }

    let header_struct = if params_by_location.has_header_params() {
      let struct_name = format!("{name}{HEADER_PARAMS_SUFFIX}");
      let fields: Vec<FieldDef> = params_by_location.header.iter().map(|(f, _, _)| f.clone()).collect();
      let struct_def = StructDef {
        name: StructToken::from_raw(&struct_name),
        docs: vec![],
        fields,
        kind: StructKind::HeaderParams,
        ..Default::default()
      };
      Some(struct_def)
    } else {
      None
    };

    if let Some(ref header_struct) = header_struct {
      main_fields.push(FieldDef {
        name: FieldNameToken::new(HEADER_PARAMS_FIELD),
        rust_type: TypeRef::new(header_struct.name.to_string()),
        ..Default::default()
      });
      nested_structs.push(header_struct.clone());
    }

    if let Some(body_type_ref) = body_type
      && let Some(body_field) = self.create_body_field(operation, body_type_ref)
    {
      main_fields.push(body_field);
    }

    let docs = operation
      .description
      .as_ref()
      .or(operation.summary.as_ref())
      .map_or_else(Vec::new, |d| metadata::extract_docs(Some(d)));

    let mut methods = vec![];

    if let Some((response_enum, response_enum_def)) = response_enum_info {
      methods.push(responses::build_parse_response_method(
        response_enum,
        &response_enum_def.variants,
      ));
    }

    let main_struct = StructDef {
      name: StructToken::from_raw(name),
      docs,
      fields: main_fields,
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods,
      kind: StructKind::OperationRequest,
      ..Default::default()
    };

    Ok(GeneratedRequestStructs {
      main_struct,
      nested_structs,
      parameter_info: all_parameter_info,
      warnings,
    })
  }

  fn apply_query_serde_attributes(mut field: FieldDef, meta: &ParameterMeta) -> FieldDef {
    if field.name.as_str() != meta.original_name {
      field
        .serde_attrs
        .push(SerdeAttribute::Rename(meta.original_name.clone()));
    }

    if field.rust_type.is_array && !meta.explode {
      let separator = match meta.style {
        Some(ParameterStyle::SpaceDelimited) => SerdeAsSeparator::Space,
        Some(ParameterStyle::PipeDelimited) => SerdeAsSeparator::Pipe,
        _ => SerdeAsSeparator::Comma,
      };
      field.serde_as_attr = Some(SerdeAsFieldAttr::SeparatedList {
        separator,
        optional: field.rust_type.nullable,
      });
    }

    field
  }

  fn prepare_request_body(
    &self,
    operation: &Operation,
    path: &str,
    usage: &mut TypeUsageRecorder,
    schema_cache: &mut SharedSchemaCache,
  ) -> anyhow::Result<RequestBodyInfo> {
    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok(RequestBodyInfo::empty(true));
    };

    let body = body_ref.resolve(self.spec)?;
    let is_required = body.required.unwrap_or(false);

    let Some((content_type_key, media_type)) = body.content.iter().next() else {
      return Ok(RequestBodyInfo::empty(!is_required));
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(RequestBodyInfo::empty(!is_required));
    };

    let output = self.resolve_request_body_type(schema_ref, path, schema_cache)?;

    let Some(output) = output else {
      return Ok(RequestBodyInfo::empty(!is_required));
    };

    usage.mark_request_iter(&output.type_usage);

    Ok(RequestBodyInfo {
      body_type: Some(output.body_type),
      generated_types: output.generated_types,
      type_usage: output.type_usage,
      field_name: Some(FieldNameToken::new(BODY_FIELD_NAME)),
      optional: !is_required,
      content_type: Some(content_type_key.clone()),
    })
  }

  fn resolve_request_body_type(
    &self,
    schema_ref: &ObjectOrReference<ObjectSchema>,
    path: &str,
    cache: &mut SharedSchemaCache,
  ) -> anyhow::Result<Option<RequestBodyOutput>> {
    match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(target_name) = SchemaRegistry::extract_ref_name(ref_path) else {
          return Ok(None);
        };
        let rust_name = to_rust_type_name(&target_name);
        Ok(Some(RequestBodyOutput {
          body_type: TypeRef::new(rust_name.clone()),
          generated_types: vec![],
          type_usage: vec![rust_name],
        }))
      }
      ObjectOrReference::Object(schema) => {
        let base_name = naming::infer_name_from_context(schema, path, REQUEST_BODY_SUFFIX);
        let Some(output) = self.schema_converter.convert_inline_schema(schema, &base_name, cache)? else {
          return Ok(None);
        };
        Ok(Some(RequestBodyOutput {
          body_type: TypeRef::new(output.type_name.clone()),
          generated_types: output.generated_types,
          type_usage: vec![output.type_name],
        }))
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

    Self::synthesize_missing_path_params(path, &mut params);

    params
  }

  /// Adds synthesized parameters for any path template variables not declared in the spec.
  fn synthesize_missing_path_params(path: &str, params: &mut Vec<Parameter>) {
    let declared: HashSet<&str> = params
      .iter()
      .filter(|p| p.location == ParameterIn::Path)
      .map(|p| p.name.as_str())
      .collect();

    let missing: Vec<_> = ParsedPath::extract_template_params(path)
      .filter(|name| !declared.contains(name))
      .map(Self::synthesize_string_path_param)
      .collect();

    params.extend(missing);
  }

  /// Creates a path parameter with String type for undeclared template variables.
  fn synthesize_string_path_param(name: &str) -> Parameter {
    Parameter {
      name: name.to_string(),
      location: ParameterIn::Path,
      description: None,
      required: Some(true),
      deprecated: None,
      allow_empty_value: None,
      style: None,
      explode: None,
      allow_reserved: None,
      schema: Some(ObjectOrReference::Object(ObjectSchema {
        schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
        ..Default::default()
      })),
      example: None,
      examples: std::collections::BTreeMap::new(),
      content: None,
      extensions: std::collections::BTreeMap::new(),
    }
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

  fn convert_parameter_with_meta(
    &self,
    param: &Parameter,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<(FieldDef, OperationParameter, ParameterMeta)> {
    let (field, meta) = self.convert_parameter(param, warnings)?;

    let param_meta = ParameterMeta {
      original_name: param.name.clone(),
      explode: path_renderer::query_param_explode(param),
      style: param.style,
    };

    Ok((field, meta, param_meta))
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
    let type_ref = self.schema_converter.resolve_type(&schema)?;
    let is_required = param.required.unwrap_or(false);
    let extractor = metadata::MetadataExtractor::new(&param.name, is_required, &schema, &type_ref);
    let validation = extractor.extract_all_validation();
    let default = extractor.extract_default_value();

    Ok((type_ref, validation, default))
  }
}
