use std::collections::HashSet;

use http::Method;
use oas3::{
  Spec,
  spec::{
    ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, ParameterStyle, SchemaType, SchemaTypeSet,
  },
};
use serde_json::Value;

use super::{
  SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache, fields::FieldConverter, path_renderer, responses,
};
use crate::generator::{
  ast::{
    ContentCategory, Documentation, EnumToken, FieldDef, FieldNameToken, OperationBody, OperationInfo, OperationKind,
    OperationParameter, OuterAttr, ParameterLocation, ParsedPath, ResponseEnumDef, ResponseMediaType, RustType,
    SerdeAsFieldAttr, SerdeAsSeparator, SerdeAttribute, StructDef, StructKind, StructToken, TypeRef,
    ValidationAttribute,
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

pub(crate) struct ConversionResult {
  pub(crate) types: Vec<RustType>,
  pub(crate) operation_info: OperationInfo,
}

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

struct RequestBodyOutput {
  body_type: TypeRef,
  generated_types: Vec<RustType>,
  type_usage: Vec<String>,
}

struct ConvertedParameter {
  field: FieldDef,
  operation_param: OperationParameter,
  serde_meta: ParameterSerdeMeta,
  inline_types: Vec<RustType>,
}

struct ParameterSerdeMeta {
  original_name: String,
  explode: bool,
  style: Option<ParameterStyle>,
}

struct ResolvedParameterType {
  type_ref: TypeRef,
  validation_attrs: Vec<ValidationAttribute>,
  default_value: Option<Value>,
  inline_types: Vec<RustType>,
}

struct ParameterGroup {
  field: FieldDef,
  serde_meta: ParameterSerdeMeta,
}

struct ParametersByLocation {
  path: Vec<ParameterGroup>,
  query: Vec<ParameterGroup>,
  header: Vec<ParameterGroup>,
}

impl ParametersByLocation {
  fn new() -> Self {
    Self {
      path: vec![],
      query: vec![],
      header: vec![],
    }
  }
}

struct GeneratedRequestStructs {
  main_struct: StructDef,
  nested_structs: Vec<StructDef>,
  inline_types: Vec<RustType>,
  parameter_info: Vec<OperationParameter>,
  warnings: Vec<String>,
}

struct ParameterStructNames {
  path: String,
  query: String,
  header: String,
}

impl ParameterStructNames {
  fn from_request_name(request_name: &str) -> Self {
    Self {
      path: format!("{request_name}{PATH_PARAMS_SUFFIX}"),
      query: format!("{request_name}{QUERY_PARAMS_SUFFIX}"),
      header: format!("{request_name}{HEADER_PARAMS_SUFFIX}"),
    }
  }

  fn parent_for_location(&self, location: ParameterIn) -> &str {
    match location {
      ParameterIn::Path => &self.path,
      ParameterIn::Query | ParameterIn::Cookie => &self.query,
      ParameterIn::Header => &self.header,
    }
  }
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
  ) -> anyhow::Result<ConversionResult> {
    let base_name = to_rust_type_name(operation_id);

    let mut warnings = vec![];
    let mut types = vec![];

    let body_info = self.prepare_request_body(operation, path, usage, schema_cache)?;
    types.extend(body_info.generated_types.clone());
    usage.mark_request_iter(&body_info.type_usage);

    let mut response_enum_def = self.build_response_enum(&base_name, operation, path, schema_cache);

    let response_enum_info = response_enum_def
      .as_ref()
      .map(|def| (EnumToken::new(def.name.to_string()), def));

    let request_name = generate_unique_request_name(&base_name, |name| self.schema_converter.is_schema_name(name));
    let generated_structs = self.build_request_struct(
      &request_name,
      path,
      operation,
      body_info.body_type.clone(),
      response_enum_info.as_ref().map(|(t, d)| (t, *d)),
      schema_cache,
    )?;

    warnings.extend(generated_structs.warnings.clone());
    let parameter_metadata = generated_structs.parameter_info.clone();

    let request_type_name = Self::emit_request_types(&mut types, generated_structs, response_enum_def.is_some(), usage);

    if let Some(def) = response_enum_def.as_mut() {
      def.request_type.clone_from(&request_type_name);
    }

    let response_enum = Self::emit_response_enum(&mut types, response_enum_def, usage);

    let response_metadata = self.extract_response_metadata(operation, usage);

    let body_metadata = Self::extract_body_metadata(&body_info);

    let final_operation_id = operation.operation_id.clone().unwrap_or(base_name);
    let parsed_path = ParsedPath::parse(path, &parameter_metadata)?;

    let operation_info = OperationInfo {
      stable_id: stable_id.to_string(),
      operation_id: final_operation_id,
      method: method.clone(),
      path: parsed_path,
      path_template: path.to_string(),
      kind,
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_metadata.type_name,
      response_enum,
      response_media_types: response_metadata.media_types,
      success_response_types: response_metadata.success_types,
      error_response_types: response_metadata.error_types,
      warnings,
      parameters: parameter_metadata,
      body: body_metadata,
    };

    Ok(ConversionResult { types, operation_info })
  }

  fn build_response_enum(
    &self,
    base_name: &str,
    operation: &Operation,
    path: &str,
    schema_cache: &mut SharedSchemaCache,
  ) -> Option<ResponseEnumDef> {
    operation.responses.as_ref()?;

    let response_name = generate_unique_response_name(base_name, |name| self.schema_converter.is_schema_name(name));

    responses::build_response_enum(
      self.schema_converter,
      self.spec,
      &response_name,
      operation,
      path,
      schema_cache,
    )
  }

  fn emit_request_types(
    types: &mut Vec<RustType>,
    generated: GeneratedRequestStructs,
    has_response_enum: bool,
    usage: &mut TypeUsageRecorder,
  ) -> Option<StructToken> {
    let has_fields = !generated.main_struct.fields.is_empty();
    let should_generate = has_fields || has_response_enum;

    if !should_generate {
      return None;
    }

    types.extend(generated.inline_types);
    types.extend(generated.nested_structs.into_iter().map(RustType::Struct));

    let request_name = generated.main_struct.name.clone();
    usage.mark_request(request_name.clone());
    types.push(RustType::Struct(generated.main_struct));

    Some(request_name)
  }

  fn emit_response_enum(
    types: &mut Vec<RustType>,
    response_enum_def: Option<ResponseEnumDef>,
    usage: &mut TypeUsageRecorder,
  ) -> Option<EnumToken> {
    let def = response_enum_def?;

    usage.mark_response(def.name.clone());
    for variant in &def.variants {
      if let Some(schema_type) = &variant.schema_type {
        usage.mark_response_type_ref(schema_type);
      }
    }

    let enum_token = EnumToken::new(def.name.to_string());
    types.push(RustType::ResponseEnum(def));

    Some(enum_token)
  }

  fn extract_body_metadata(body_info: &RequestBodyInfo) -> Option<OperationBody> {
    let field_name = body_info.field_name.as_ref()?;
    let content_category = body_info
      .content_type
      .as_deref()
      .map_or(ContentCategory::Json, ContentCategory::from_content_type);

    Some(OperationBody {
      field_name: field_name.clone(),
      optional: body_info.optional,
      content_category,
    })
  }

  fn build_request_struct(
    &self,
    name: &str,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
    response_enum_info: Option<(&EnumToken, &ResponseEnumDef)>,
    cache: &mut SharedSchemaCache,
  ) -> anyhow::Result<GeneratedRequestStructs> {
    let mut warnings = vec![];
    let mut inline_types = vec![];

    let struct_names = ParameterStructNames::from_request_name(name);

    let (params_by_location, all_parameter_info, param_inline_types) =
      self.collect_and_convert_parameters(path, operation, &struct_names, cache, &mut warnings)?;

    inline_types.extend(param_inline_types);

    let mut nested_structs = vec![];
    let mut main_fields = vec![];

    Self::add_path_params_struct(
      &params_by_location,
      &struct_names,
      &mut main_fields,
      &mut nested_structs,
    );
    Self::add_query_params_struct(
      &params_by_location,
      &struct_names,
      &mut main_fields,
      &mut nested_structs,
    );
    Self::add_header_params_struct(
      &params_by_location,
      &struct_names,
      &mut main_fields,
      &mut nested_structs,
    );

    if let Some(body_type_ref) = body_type
      && let Some(body_field) = self.create_body_field(operation, body_type_ref)
    {
      main_fields.push(body_field);
    }

    let docs = Documentation::from_optional(operation.description.as_ref().or(operation.summary.as_ref()));

    let methods = response_enum_info
      .map(|(enum_token, def)| vec![responses::build_parse_response_method(enum_token, &def.variants)])
      .unwrap_or_default();

    let main_struct = StructDef {
      name: StructToken::from_raw(name),
      docs,
      fields: main_fields,
      methods,
      kind: StructKind::OperationRequest,
      ..Default::default()
    };

    Ok(GeneratedRequestStructs {
      main_struct,
      nested_structs,
      inline_types,
      parameter_info: all_parameter_info,
      warnings,
    })
  }

  fn collect_and_convert_parameters(
    &self,
    path: &str,
    operation: &Operation,
    struct_names: &ParameterStructNames,
    cache: &mut SharedSchemaCache,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<(ParametersByLocation, Vec<OperationParameter>, Vec<RustType>)> {
    let mut params_by_location = ParametersByLocation::new();
    let mut all_parameter_info = vec![];
    let mut inline_types = vec![];

    for param in self.collect_parameters(path, operation) {
      let parent_struct_name = struct_names.parent_for_location(param.location);

      let converted = self.convert_parameter(&param, parent_struct_name, cache, warnings)?;
      all_parameter_info.push(converted.operation_param.clone());
      inline_types.extend(converted.inline_types);

      let group = ParameterGroup {
        field: converted.field,
        serde_meta: converted.serde_meta,
      };

      match param.location {
        ParameterIn::Path => params_by_location.path.push(group),
        ParameterIn::Query => params_by_location.query.push(group),
        ParameterIn::Header => params_by_location.header.push(group),
        ParameterIn::Cookie => {}
      }
    }

    Ok((params_by_location, all_parameter_info, inline_types))
  }

  fn add_path_params_struct(
    params: &ParametersByLocation,
    names: &ParameterStructNames,
    main_fields: &mut Vec<FieldDef>,
    nested_structs: &mut Vec<StructDef>,
  ) {
    if params.path.is_empty() {
      return;
    }

    let fields: Vec<FieldDef> = params.path.iter().map(|g| g.field.clone()).collect();

    let struct_def = StructDef {
      name: StructToken::from_raw(&names.path),
      fields,
      kind: StructKind::PathParams,
      ..Default::default()
    };

    main_fields.push(FieldDef {
      name: FieldNameToken::new(PATH_PARAMS_FIELD),
      rust_type: TypeRef::new(struct_def.name.to_string()),
      ..Default::default()
    });

    nested_structs.push(struct_def);
  }

  fn add_query_params_struct(
    params: &ParametersByLocation,
    names: &ParameterStructNames,
    main_fields: &mut Vec<FieldDef>,
    nested_structs: &mut Vec<StructDef>,
  ) {
    if params.query.is_empty() {
      return;
    }

    let fields: Vec<FieldDef> = params
      .query
      .iter()
      .map(|g| Self::apply_query_serde_attributes(g.field.clone(), &g.serde_meta))
      .collect();

    let has_serde_as = fields.iter().any(|f| f.serde_as_attr.is_some());
    let outer_attrs = if has_serde_as { vec![OuterAttr::SerdeAs] } else { vec![] };

    let struct_def = StructDef {
      name: StructToken::from_raw(&names.query),
      fields,
      outer_attrs,
      kind: StructKind::QueryParams,
      ..Default::default()
    };

    main_fields.push(FieldDef {
      name: FieldNameToken::new(QUERY_PARAMS_FIELD),
      rust_type: TypeRef::new(struct_def.name.to_string()),
      ..Default::default()
    });

    nested_structs.push(struct_def);
  }

  fn add_header_params_struct(
    params: &ParametersByLocation,
    names: &ParameterStructNames,
    main_fields: &mut Vec<FieldDef>,
    nested_structs: &mut Vec<StructDef>,
  ) {
    if params.header.is_empty() {
      return;
    }

    let fields: Vec<FieldDef> = params.header.iter().map(|g| g.field.clone()).collect();

    let struct_def = StructDef {
      name: StructToken::from_raw(&names.header),
      fields,
      kind: StructKind::HeaderParams,
      ..Default::default()
    };

    main_fields.push(FieldDef {
      name: FieldNameToken::new(HEADER_PARAMS_FIELD),
      rust_type: TypeRef::new(struct_def.name.to_string()),
      ..Default::default()
    });

    nested_structs.push(struct_def);
  }

  fn apply_query_serde_attributes(mut field: FieldDef, meta: &ParameterSerdeMeta) -> FieldDef {
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

    let Some(output) = self.resolve_request_body_type(schema_ref, path, schema_cache)? else {
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
      ObjectOrReference::Ref { ref_path, .. } => Ok(Self::resolve_referenced_body_type(ref_path)),
      ObjectOrReference::Object(schema) => self.resolve_inline_body_type(schema, path, cache),
    }
  }

  fn resolve_referenced_body_type(ref_path: &str) -> Option<RequestBodyOutput> {
    let target_name = SchemaRegistry::extract_ref_name(ref_path)?;
    let rust_name = to_rust_type_name(&target_name);

    Some(RequestBodyOutput {
      body_type: TypeRef::new(rust_name.clone()),
      generated_types: vec![],
      type_usage: vec![rust_name],
    })
  }

  fn resolve_inline_body_type(
    &self,
    schema: &ObjectSchema,
    path: &str,
    cache: &mut SharedSchemaCache,
  ) -> anyhow::Result<Option<RequestBodyOutput>> {
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

  fn create_body_field(&self, operation: &Operation, body_type: TypeRef) -> Option<FieldDef> {
    let body_ref = operation.request_body.as_ref()?;
    let body = body_ref.resolve(self.spec).ok()?;
    let is_required = body.required.unwrap_or(false);

    let docs = Documentation::from_optional(body.description.as_ref());

    let rust_type = if is_required {
      body_type
    } else {
      body_type.with_option()
    };

    Some(FieldDef {
      name: FieldNameToken::new(BODY_FIELD_NAME),
      docs,
      rust_type,
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
    parent_struct_name: &str,
    cache: &mut SharedSchemaCache,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<ConvertedParameter> {
    let resolved = self.resolve_parameter_type(param, parent_struct_name, cache, warnings)?;

    let is_required = param.required.unwrap_or(false);
    let docs = Documentation::from_optional(param.description.as_ref());

    let final_rust_type = if is_required {
      resolved.type_ref.clone()
    } else {
      resolved.type_ref.clone().with_option()
    };

    let rust_field = FieldNameToken::new(to_rust_field_name(&param.name));

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
      validation_attrs: resolved.validation_attrs,
      default_value: resolved.default_value,
      example_value: param.example.clone(),
      parameter_location: Some(location),
      ..Default::default()
    };

    let operation_param = OperationParameter {
      original_name: param.name.clone(),
      rust_field,
      location,
      required: is_required,
      rust_type: final_rust_type,
    };

    let serde_meta = ParameterSerdeMeta {
      original_name: param.name.clone(),
      explode: path_renderer::query_param_explode(param),
      style: param.style,
    };

    Ok(ConvertedParameter {
      field,
      operation_param,
      serde_meta,
      inline_types: resolved.inline_types,
    })
  }

  fn resolve_parameter_type(
    &self,
    param: &Parameter,
    parent_struct_name: &str,
    cache: &mut SharedSchemaCache,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<ResolvedParameterType> {
    let Some(schema_ref) = param.schema.as_ref() else {
      warnings.push(format!(
        "Parameter '{}' has no schema, defaulting to String.",
        param.name
      ));
      return Ok(ResolvedParameterType {
        type_ref: TypeRef::new("String"),
        validation_attrs: vec![],
        default_value: None,
        inline_types: vec![],
      });
    };

    let schema = schema_ref.resolve(self.spec)?;
    let has_inline_enum = schema.enum_values.len() > 1;

    let (type_ref, inline_types) = if has_inline_enum {
      let result = self.schema_converter.resolve_property_type(
        parent_struct_name,
        &param.name,
        &schema,
        schema_ref,
        Some(cache),
      )?;
      (result.result, result.inline_types)
    } else {
      (self.schema_converter.resolve_type(&schema)?, vec![])
    };

    let is_required = param.required.unwrap_or(false);
    let (validation_attrs, default_value) =
      FieldConverter::extract_parameter_metadata(&param.name, is_required, &schema, &type_ref);

    Ok(ResolvedParameterType {
      type_ref,
      validation_attrs,
      default_value,
      inline_types,
    })
  }
}

struct ResponseMetadata {
  type_name: Option<String>,
  media_types: Vec<ResponseMediaType>,
  success_types: Vec<String>,
  error_types: Vec<String>,
}

impl OperationConverter<'_> {
  fn extract_response_metadata(&self, operation: &Operation, usage: &mut TypeUsageRecorder) -> ResponseMetadata {
    let type_name = naming_responses::extract_response_type_name(self.spec, operation);

    let media_types = naming_responses::extract_all_response_content_types(self.spec, operation)
      .into_iter()
      .map(|ct| ResponseMediaType::new(&ct))
      .collect::<Vec<_>>();

    let media_types = if media_types.is_empty() {
      vec![ResponseMediaType::new("application/json")]
    } else {
      media_types
    };

    let response_types = naming_responses::extract_all_response_types(self.spec, operation);

    if let Some(name) = &type_name {
      usage.mark_response(name);
    }
    usage.mark_response_iter(&response_types.success);
    usage.mark_response_iter(&response_types.error);

    ResponseMetadata {
      type_name,
      media_types,
      success_types: response_types.success,
      error_types: response_types.error,
    }
  }
}
