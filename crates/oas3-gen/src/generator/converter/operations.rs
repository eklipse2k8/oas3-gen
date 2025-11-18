use std::collections::HashSet;

use http::Method;
use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn},
};
use serde_json::Value;

use super::{
  BODY_FIELD_NAME, ConversionResult, DEFAULT_RESPONSE_DESCRIPTION, DEFAULT_RESPONSE_VARIANT, REQUEST_BODY_SUFFIX,
  REQUEST_PARAMS_SUFFIX, REQUEST_SUFFIX, RESPONSE_ENUM_SUFFIX, RESPONSE_SUFFIX, STATUS_ACCEPTED, STATUS_BAD_GATEWAY,
  STATUS_BAD_REQUEST, STATUS_CLIENT_ERROR, STATUS_CONFLICT, STATUS_CREATED, STATUS_FORBIDDEN, STATUS_FOUND,
  STATUS_GATEWAY_TIMEOUT, STATUS_GONE, STATUS_INFORMATIONAL, STATUS_INTERNAL_SERVER_ERROR, STATUS_METHOD_NOT_ALLOWED,
  STATUS_MOVED_PERMANENTLY, STATUS_NO_CONTENT, STATUS_NOT_ACCEPTABLE, STATUS_NOT_FOUND, STATUS_NOT_IMPLEMENTED,
  STATUS_NOT_MODIFIED, STATUS_OK, STATUS_PREFIX, STATUS_REDIRECTION, STATUS_REQUEST_TIMEOUT, STATUS_SERVER_ERROR,
  STATUS_SERVICE_UNAVAILABLE, STATUS_SUCCESS, STATUS_TOO_MANY_REQUESTS, STATUS_UNAUTHORIZED,
  STATUS_UNPROCESSABLE_ENTITY, SUCCESS_RESPONSE_PREFIX, SchemaConverter, TypeUsageRecorder, cache::SharedSchemaCache,
  metadata,
};
use crate::{
  generator::{
    ast::{
      FieldDef, OperationBody, OperationInfo, OperationParameter, ParameterLocation, PathSegment, QueryParameter,
      ResponseEnumDef, ResponseVariant, RustType, StructDef, StructKind, TypeAliasDef, TypeRef,
    },
    schema_graph::SchemaGraph,
  },
  reserved::{to_rust_field_name, to_rust_type_name},
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
      self
        .build_response_enum(&response_name, None, operation, path, schema_cache)
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

    let response_type_name = self.extract_response_type_name(operation);
    let response_content_type = self.extract_response_content_type(operation);
    let (success_response_types, error_response_types) = self.extract_all_response_types(operation);
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
      methods.push(Self::build_parse_response_method(
        response_enum_name,
        &response_enum_def.variants,
      ));
    }

    let struct_def = StructDef {
      name: to_rust_type_name(name),
      docs,
      fields,
      derives: vec!["Debug".into(), "Clone".into(), "oas3_gen_support::Default".into()],
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

        let cached_type_name = ctx.schema_cache.get_or_create_type(
          inline_schema,
          self.schema_converter,
          path,
          "RequestBody",
          StructKind::RequestBody,
        )?;

        ctx.type_usage.push(cached_type_name.clone());
        ctx.usage.mark_request(&cached_type_name);
        Ok(Some(TypeRef::new(cached_type_name)))
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
    let validation = SchemaConverter::extract_validation_attrs(&param.name, is_required, &schema);
    let regex = SchemaConverter::extract_validation_pattern(&param.name, &schema).cloned();
    let default = SchemaConverter::extract_default_value(&schema);

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
        optional: field.rust_type.nullable,
        is_array: field.rust_type.is_array,
      }),
      _ => {}
    }
  }

  fn extract_response_type_name(&self, operation: &Operation) -> Option<String> {
    let responses = operation.responses.as_ref()?;
    responses
      .iter()
      .find(|(code, _)| code.starts_with(SUCCESS_RESPONSE_PREFIX))
      .or_else(|| responses.iter().next())
      .and_then(|(_, resp_ref)| resp_ref.resolve(self.spec).ok())
      .and_then(|resp| Self::extract_schema_name_from_response(&resp))
      .map(|s| to_rust_type_name(&s))
  }

  fn extract_response_content_type(&self, operation: &Operation) -> Option<String> {
    let responses = operation.responses.as_ref()?;
    responses
      .iter()
      .find(|(code, _)| code.starts_with(SUCCESS_RESPONSE_PREFIX))
      .or_else(|| responses.iter().next())
      .and_then(|(_, resp_ref)| resp_ref.resolve(self.spec).ok())
      .and_then(|resp| resp.content.keys().next().cloned())
  }

  fn extract_all_response_types(&self, operation: &Operation) -> (Vec<String>, Vec<String>) {
    let mut success_set = HashSet::new();
    let mut error_set = HashSet::new();

    let Some(responses) = operation.responses.as_ref() else {
      return (Vec::new(), Vec::new());
    };

    for (code, resp_ref) in responses {
      if let Ok(resp) = resp_ref.resolve(self.spec)
        && let Some(schema_name) = Self::extract_schema_name_from_response(&resp)
      {
        let rust_name = to_rust_type_name(&schema_name);
        if Self::is_success_code(code) {
          success_set.insert(rust_name);
        } else if Self::is_error_code(code) {
          error_set.insert(rust_name);
        }
      }
    }

    (success_set.into_iter().collect(), error_set.into_iter().collect())
  }

  fn is_success_code(code: &str) -> bool {
    code.starts_with('2')
  }

  fn is_error_code(code: &str) -> bool {
    code.starts_with('4') || code.starts_with('5')
  }

  fn extract_schema_name_from_response(response: &oas3::spec::Response) -> Option<String> {
    response
      .content
      .values()
      .next()?
      .schema
      .as_ref()
      .and_then(|schema_ref| match schema_ref {
        ObjectOrReference::Ref { ref_path, .. } => SchemaGraph::extract_ref_name(ref_path),
        ObjectOrReference::Object(_) => None,
      })
  }

  fn build_response_enum(
    &self,
    name: &str,
    request_type: Option<&String>,
    operation: &Operation,
    path: &str,
    schema_cache: &mut SharedSchemaCache,
  ) -> Option<ResponseEnumDef> {
    let responses = operation.responses.as_ref()?;

    let mut variants = Vec::new();
    let base_name = to_rust_type_name(name);

    for (status_code, resp_ref) in responses {
      let Ok(response) = resp_ref.resolve(self.spec) else {
        continue;
      };

      let variant_name = Self::status_code_to_variant_name(status_code, &response);
      let schema_type = self
        .extract_response_schema_type(&response, path, status_code, schema_cache)
        .ok()
        .flatten();

      variants.push(ResponseVariant {
        status_code: status_code.clone(),
        variant_name,
        description: response.description.clone(),
        schema_type,
      });
    }

    if variants.is_empty() {
      return None;
    }

    let has_default = variants.iter().any(|v| v.status_code == "default");
    if !has_default {
      variants.push(ResponseVariant {
        status_code: "default".to_string(),
        variant_name: DEFAULT_RESPONSE_VARIANT.to_string(),
        description: Some(DEFAULT_RESPONSE_DESCRIPTION.to_string()),
        schema_type: None,
      });
    }

    Some(ResponseEnumDef {
      name: base_name,
      docs: vec![format!("/// Response types for {}", operation.operation_id.as_ref()?)],
      variants,
      request_type: request_type.map_or_else(String::new, std::clone::Clone::clone),
    })
  }

  fn status_code_to_variant_name(status_code: &str, response: &oas3::spec::Response) -> String {
    match status_code {
      "200" => STATUS_OK.to_string(),
      "201" => STATUS_CREATED.to_string(),
      "202" => STATUS_ACCEPTED.to_string(),
      "204" => STATUS_NO_CONTENT.to_string(),
      "301" => STATUS_MOVED_PERMANENTLY.to_string(),
      "302" => STATUS_FOUND.to_string(),
      "304" => STATUS_NOT_MODIFIED.to_string(),
      "400" => STATUS_BAD_REQUEST.to_string(),
      "401" => STATUS_UNAUTHORIZED.to_string(),
      "403" => STATUS_FORBIDDEN.to_string(),
      "404" => STATUS_NOT_FOUND.to_string(),
      "405" => STATUS_METHOD_NOT_ALLOWED.to_string(),
      "406" => STATUS_NOT_ACCEPTABLE.to_string(),
      "408" => STATUS_REQUEST_TIMEOUT.to_string(),
      "409" => STATUS_CONFLICT.to_string(),
      "410" => STATUS_GONE.to_string(),
      "422" => STATUS_UNPROCESSABLE_ENTITY.to_string(),
      "429" => STATUS_TOO_MANY_REQUESTS.to_string(),
      "500" => STATUS_INTERNAL_SERVER_ERROR.to_string(),
      "501" => STATUS_NOT_IMPLEMENTED.to_string(),
      "502" => STATUS_BAD_GATEWAY.to_string(),
      "503" => STATUS_SERVICE_UNAVAILABLE.to_string(),
      "504" => STATUS_GATEWAY_TIMEOUT.to_string(),
      "1XX" => STATUS_INFORMATIONAL.to_string(),
      "2XX" => STATUS_SUCCESS.to_string(),
      "3XX" => STATUS_REDIRECTION.to_string(),
      "4XX" => STATUS_CLIENT_ERROR.to_string(),
      "5XX" => STATUS_SERVER_ERROR.to_string(),
      code if code.ends_with("XX") || code.ends_with("xx") => {
        let prefix = &code[0..1];
        match prefix {
          "1" => STATUS_INFORMATIONAL.to_string(),
          "2" => STATUS_SUCCESS.to_string(),
          "3" => STATUS_REDIRECTION.to_string(),
          "4" => STATUS_CLIENT_ERROR.to_string(),
          "5" => STATUS_SERVER_ERROR.to_string(),
          _ => format!("{STATUS_PREFIX}{}", code.replace(['X', 'x'], "")),
        }
      }
      code => {
        if let Some(desc) = &response.description {
          let sanitized = desc
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>();
          let words: Vec<&str> = sanitized.split_whitespace().take(3).collect();
          if !words.is_empty() {
            return to_rust_type_name(&words.join("_"));
          }
        }
        format!("{STATUS_PREFIX}{code}")
      }
    }
  }

  fn extract_response_schema_type(
    &self,
    response: &oas3::spec::Response,
    path: &str,
    status_code: &str,
    schema_cache: &mut SharedSchemaCache,
  ) -> ConversionResult<Option<TypeRef>> {
    let Some(media_type) = response.content.values().next() else {
      return Ok(None);
    };
    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(None);
    };

    match schema_ref {
      ObjectOrReference::Ref { ref_path, .. } => {
        let Some(schema_name) = SchemaGraph::extract_ref_name(ref_path) else {
          return Ok(None);
        };
        Ok(Some(TypeRef::new(to_rust_type_name(&schema_name))))
      }
      ObjectOrReference::Object(inline_schema) => {
        if inline_schema.properties.is_empty() && inline_schema.schema_type.is_none() {
          return Ok(None);
        }

        let rust_type_name = schema_cache.get_or_create_type(
          inline_schema,
          self.schema_converter,
          path,
          status_code,
          StructKind::Schema,
        )?;

        Ok(Some(TypeRef::new(rust_type_name)))
      }
    }
  }

  fn build_parse_response_method(
    response_enum_name: &str,
    variants: &[ResponseVariant],
  ) -> crate::generator::ast::StructMethod {
    use crate::generator::ast::{StructMethod, StructMethodKind};

    StructMethod {
      name: "parse_response".to_string(),
      docs: vec!["/// Parse the HTTP response into the response enum.".to_string()],
      kind: StructMethodKind::ParseResponse {
        response_enum: response_enum_name.to_string(),
        variants: variants.to_vec(),
      },
      attrs: vec![],
    }
  }
}

mod path_renderer {
  use std::collections::HashMap;

  use oas3::spec::{Parameter, ParameterStyle};
  use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

  use super::{PathSegment, QueryParameter};
  use crate::generator::ast::StructMethod;

  const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

  #[derive(Debug, Clone)]
  pub(super) struct PathParamMapping {
    pub rust_field: String,
    pub original_name: String,
  }

  #[derive(Debug, Clone)]
  pub(super) struct QueryParamMapping {
    pub rust_field: String,
    pub original_name: String,
    pub explode: bool,
    pub optional: bool,
    pub is_array: bool,
  }

  pub(super) fn build_render_path_method(
    path: &str,
    path_params: &[PathParamMapping],
    query_params: &[QueryParamMapping],
  ) -> StructMethod {
    use crate::generator::ast::StructMethodKind;

    let query_parameters = query_params
      .iter()
      .map(|m| QueryParameter {
        field: m.rust_field.clone(),
        encoded_name: encode_query_name(&m.original_name),
        explode: m.explode,
        optional: m.optional,
        is_array: m.is_array,
      })
      .collect();

    StructMethod {
      name: "render_path".to_string(),
      docs: vec!["/// Render the request path with parameters.".to_string()],
      kind: StructMethodKind::RenderPath {
        segments: parse_path_segments(path, path_params),
        query_params: query_parameters,
      },
      attrs: vec![],
    }
  }

  fn parse_path_segments(path: &str, path_params: &[PathParamMapping]) -> Vec<PathSegment> {
    if path_params.is_empty() {
      return vec![PathSegment::Literal(path.to_string())];
    }
    let param_map: HashMap<_, _> = path_params
      .iter()
      .map(|p| (&*p.original_name, &*p.rust_field))
      .collect();
    let mut segments = Vec::new();
    let mut last_end = 0;
    for (start, _part) in path.match_indices('{') {
      if start > last_end {
        segments.push(PathSegment::Literal(path[last_end..start].to_string()));
      }
      let end = path[start..].find('}').map_or(path.len(), |i| start + i);
      let param_name = &path[start + 1..end];
      if let Some(rust_field) = param_map.get(param_name) {
        segments.push(PathSegment::Parameter {
          field: (*rust_field).to_string(),
        });
      } else {
        segments.push(PathSegment::Literal(path[start..=end].to_string()));
      }
      last_end = end + 1;
    }
    if last_end < path.len() {
      segments.push(PathSegment::Literal(path[last_end..].to_string()));
    }
    segments
  }

  pub(super) fn encode_query_name(name: &str) -> String {
    utf8_percent_encode(name, QUERY_ENCODE_SET).to_string()
  }

  pub(super) fn query_param_explode(param: &Parameter) -> bool {
    param
      .explode
      .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)))
  }
}
