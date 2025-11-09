use std::collections::{HashMap, HashSet};

use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn},
};

use super::{SchemaConverter, error::ConversionResult, metadata};
use crate::{
  generator::{
    ast::{
      FieldDef, OperationInfo, PathSegment, QueryParameter, ResponseEnumDef, ResponseVariant, RustType, StructDef,
      StructKind, TypeAliasDef, TypeRef,
    },
    schema_graph::SchemaGraph,
  },
  reserved::{to_rust_field_name, to_rust_type_name},
};

const REQUEST_SUFFIX: &str = "Request";
const REQUEST_BODY_SUFFIX: &str = "RequestBody";
const RESPONSE_SUFFIX: &str = "Response";
const BODY_FIELD_NAME: &str = "body";
const SUCCESS_RESPONSE_PREFIX: char = '2';

type ParameterValidation = (TypeRef, Vec<String>, Option<String>, Option<serde_json::Value>);

struct RequestBodyInfo {
  body_type: Option<TypeRef>,
  generated_types: Vec<RustType>,
  type_usage: Vec<String>,
}

#[derive(Default)]
struct ParameterMappings {
  path: Vec<path_renderer::PathParamMapping>,
  query: Vec<path_renderer::QueryParamMapping>,
}

struct SharedSchemaCache {
  schema_to_type: HashMap<String, String>,
  generated_types: Vec<RustType>,
  used_names: HashSet<String>,
}

impl SharedSchemaCache {
  fn new() -> Self {
    Self {
      schema_to_type: HashMap::new(),
      generated_types: Vec::new(),
      used_names: HashSet::new(),
    }
  }

  fn get_or_create_type(
    &mut self,
    schema: &ObjectSchema,
    converter: &SchemaConverter,
    path: &str,
    context: &str,
    kind: StructKind,
  ) -> ConversionResult<String> {
    let schema_hash = Self::hash_schema(schema);

    if let Some(existing_type) = self.schema_to_type.get(&schema_hash) {
      return Ok(existing_type.clone());
    }

    let base_name = Self::infer_name_from_context(schema, path, context);
    let type_name = self.make_unique_name(base_name);
    let rust_type_name = to_rust_type_name(&type_name);

    let (body_struct, mut nested_types) = converter.convert_struct(&type_name, schema, Some(kind))?;

    self.generated_types.append(&mut nested_types);
    self.generated_types.push(body_struct);
    self.schema_to_type.insert(schema_hash, rust_type_name.clone());
    self.used_names.insert(rust_type_name.clone());

    Ok(rust_type_name)
  }

  fn infer_name_from_context(schema: &ObjectSchema, path: &str, context: &str) -> String {
    if schema.properties.len() == 1
      && let Some((prop_name, _)) = schema.properties.iter().next()
    {
      let singular = Self::singularize(prop_name);
      if context == "RequestBody" {
        return singular;
      }
      return format!("{singular}Response");
    }

    let path_segments: Vec<&str> = path
      .split('/')
      .filter(|s| !s.is_empty() && !s.starts_with('{'))
      .collect();

    if let Some(last_segment) = path_segments.last() {
      let singular = Self::singularize(last_segment);
      if context == "RequestBody" {
        return format!("{singular}RequestBody");
      }
      return format!("{singular}{context}Response");
    }

    if let Some(first_segment) = path_segments.first() {
      if context == "RequestBody" {
        return format!("{first_segment}RequestBody");
      }
      return format!("{first_segment}{context}Response");
    }

    if context == "RequestBody" {
      return "RequestBody".to_string();
    }
    format!("Response{context}")
  }

  fn singularize(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
      return format!("{stem}y");
    }
    if let Some(stem) = word.strip_suffix("sses") {
      return format!("{stem}ss");
    }
    if let Some(stem) = word.strip_suffix("xes") {
      return format!("{stem}x");
    }
    if let Some(stem) = word.strip_suffix("ches") {
      return format!("{stem}ch");
    }
    if let Some(stem) = word.strip_suffix("shes") {
      return format!("{stem}sh");
    }
    if let Some(stem) = word.strip_suffix('s')
      && !stem.is_empty()
      && !stem.ends_with('s')
    {
      return stem.to_string();
    }
    word.to_string()
  }

  fn make_unique_name(&mut self, base: String) -> String {
    let rust_name = to_rust_type_name(&base);
    if !self.used_names.contains(&rust_name) {
      return base;
    }

    let mut suffix = 2;
    while self.used_names.contains(&to_rust_type_name(&format!("{base}{suffix}"))) {
      suffix += 1;
    }
    format!("{base}{suffix}")
  }

  fn hash_schema(schema: &ObjectSchema) -> String {
    let Ok(mut value) = serde_json::to_value(schema) else {
      return String::new();
    };

    if let serde_json::Value::Object(map) = &mut value
      && let Some(serde_json::Value::Array(required_list)) = map.get_mut("required")
    {
      let mut strings: Vec<String> = required_list
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();

      if strings.len() == required_list.len() {
        strings.sort_unstable();
        *required_list = strings.into_iter().map(serde_json::Value::String).collect();
      }
    }

    serde_json::to_string(&value).unwrap_or_default()
  }

  fn into_types(self) -> Vec<RustType> {
    self.generated_types
  }
}

pub(crate) struct OperationConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
  schema_cache: std::cell::RefCell<SharedSchemaCache>,
}

impl<'a> OperationConverter<'a> {
  pub(crate) fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self {
      schema_converter,
      spec,
      schema_cache: std::cell::RefCell::new(SharedSchemaCache::new()),
    }
  }

  pub(crate) fn finish(self) -> Vec<RustType> {
    self.schema_cache.into_inner().into_types()
  }

  pub(crate) fn convert(
    &self,
    operation_id: &str,
    method: &str,
    path: &str,
    operation: &Operation,
  ) -> ConversionResult<(Vec<RustType>, OperationInfo)> {
    let base_name = to_rust_type_name(operation_id);

    let mut warnings = Vec::new();
    let mut types = Vec::new();

    let body_info = self.prepare_request_body(&base_name, operation, path)?;
    types.extend(body_info.generated_types);

    let response_enum_info = if operation.responses.is_some() {
      let response_name = format!("{base_name}{RESPONSE_SUFFIX}");
      self
        .build_response_enum(&response_name, None, operation, path)
        .map(|def| (response_name, def))
    } else {
      None
    };

    let request_type_name = if self.operation_has_parameters(path, operation) || body_info.body_type.is_some() {
      let request_name = format!("{base_name}{REQUEST_SUFFIX}");
      let (request_struct, request_warnings) = self.build_request_struct(
        &request_name,
        path,
        operation,
        body_info.body_type,
        response_enum_info.as_ref(),
      )?;

      warnings.extend(request_warnings);
      types.push(RustType::Struct(request_struct));
      Some(request_name)
    } else {
      None
    };

    let response_enum_name = if let Some((name, mut def)) = response_enum_info {
      def.request_type = request_type_name.clone().unwrap_or_default();
      types.push(RustType::ResponseEnum(def));
      Some(name)
    } else {
      None
    };

    let response_type_name = self.extract_response_type_name(operation);
    let (success_response_types, error_response_types) = self.extract_all_response_types(operation);

    let op_info = OperationInfo {
      operation_id: operation.operation_id.clone().unwrap_or(base_name),
      method: method.to_string(),
      path: path.to_string(),
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_type_name,
      response_enum: response_enum_name,
      request_body_types: body_info.type_usage,
      success_response_types,
      error_response_types,
      warnings,
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
  ) -> ConversionResult<(StructDef, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut fields = Vec::new();
    let mut param_mappings = ParameterMappings::default();

    for param in self.collect_parameters(path, operation) {
      let field = self.convert_parameter(&param, &mut warnings)?;
      Self::map_parameter(&param, &field, &mut param_mappings);
      fields.push(field);
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
      derives: vec![
        "Clone".into(),
        "Debug".into(),
        "validator::Validate".into(),
        "oas3_gen_support::Default".into(),
      ],
      serde_attrs: vec![],
      outer_attrs: vec![],
      methods,
      kind: StructKind::OperationRequest,
    };

    Ok((struct_def, warnings))
  }

  fn prepare_request_body(
    &self,
    base_name: &str,
    operation: &Operation,
    path: &str,
  ) -> ConversionResult<RequestBodyInfo> {
    let mut generated_types = Vec::new();
    let mut type_usage = Vec::new();

    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
      });
    };

    let body = body_ref.resolve(self.spec)?;
    let Some((_content_type, media_type)) = body.content.iter().next() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
      });
    };

    let Some(schema_ref) = media_type.schema.as_ref() else {
      return Ok(RequestBodyInfo {
        body_type: None,
        generated_types,
        type_usage,
      });
    };

    let raw_body_type_name = format!("{base_name}{REQUEST_BODY_SUFFIX}");
    let body_type = self.process_request_body_schema(
      schema_ref,
      &raw_body_type_name,
      &mut type_usage,
      path,
      &mut generated_types,
      body.description.as_ref(),
    )?;

    Ok(RequestBodyInfo {
      body_type,
      generated_types,
      type_usage,
    })
  }

  fn process_request_body_schema(
    &self,
    schema_ref: &ObjectOrReference<oas3::spec::ObjectSchema>,
    type_name: &str,
    type_usage: &mut Vec<String>,
    path: &str,
    generated_types: &mut Vec<RustType>,
    description: Option<&String>,
  ) -> ConversionResult<Option<TypeRef>> {
    let rust_type_name = to_rust_type_name(type_name);

    match schema_ref {
      ObjectOrReference::Object(inline_schema) => {
        if inline_schema.properties.is_empty() {
          return Ok(None);
        }

        let mut cache = self.schema_cache.borrow_mut();
        let cached_type_name = cache.get_or_create_type(
          inline_schema,
          self.schema_converter,
          path,
          "RequestBody",
          StructKind::RequestBody,
        )?;

        type_usage.push(cached_type_name.clone());
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
        generated_types.push(RustType::TypeAlias(alias));
        type_usage.push(target_rust_name);
        type_usage.push(rust_type_name.clone());
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

  fn convert_parameter(&self, param: &Parameter, warnings: &mut Vec<String>) -> ConversionResult<FieldDef> {
    let (rust_type, validation_attrs, regex_validation, default_value) =
      self.extract_parameter_type_and_validation(param, warnings)?;

    let is_required = param.required.unwrap_or(false);
    let mut docs = metadata::extract_docs(param.description.as_ref());
    docs.push(format!("/// (Location: {:?})", param.location));

    Ok(FieldDef {
      name: to_rust_field_name(&param.name),
      docs,
      rust_type: if is_required {
        rust_type
      } else {
        rust_type.with_option()
      },
      validation_attrs,
      regex_validation,
      default_value,
      ..Default::default()
    })
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

  fn operation_has_parameters(&self, path: &str, operation: &Operation) -> bool {
    !operation.parameters.is_empty()
      || self
        .spec
        .paths
        .as_ref()
        .and_then(|p| p.get(path))
        .is_some_and(|item| !item.parameters.is_empty())
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
        .extract_response_schema_type(&response, path, status_code)
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
        variant_name: "Unknown".to_string(),
        description: Some("Unknown response".to_string()),
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
      "200" => "Ok".to_string(),
      "201" => "Created".to_string(),
      "202" => "Accepted".to_string(),
      "204" => "NoContent".to_string(),
      "400" => "BadRequest".to_string(),
      "401" => "Unauthorized".to_string(),
      "403" => "Forbidden".to_string(),
      "404" => "NotFound".to_string(),
      "409" => "Conflict".to_string(),
      "500" => "InternalServerError".to_string(),
      "1XX" => "Informational".to_string(),
      "2XX" => "Success".to_string(),
      "3XX" => "Redirection".to_string(),
      "4XX" => "ClientError".to_string(),
      "5XX" => "ServerError".to_string(),
      code if code.ends_with("XX") || code.ends_with("xx") => {
        let prefix = &code[0..1];
        match prefix {
          "1" => "Informational".to_string(),
          "2" => "Success".to_string(),
          "3" => "Redirection".to_string(),
          "4" => "ClientError".to_string(),
          "5" => "ServerError".to_string(),
          _ => format!("Status{}", code.replace(['X', 'x'], "")),
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
        format!("Status{code}")
      }
    }
  }

  fn extract_response_schema_type(
    &self,
    response: &oas3::spec::Response,
    path: &str,
    status_code: &str,
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

        let mut cache = self.schema_cache.borrow_mut();
        let rust_type_name = cache.get_or_create_type(
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
      attrs: vec!["must_use".to_string()],
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
