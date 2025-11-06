use std::collections::HashMap;

use http::Method;
use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn, ParameterStyle},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::{
  ast::{
    FieldDef, OperationInfo, PathSegment, QueryParameter, RustType, StructDef, StructMethod, StructMethodKind,
    TypeAliasDef, TypeRef,
  },
  schema_converter::SchemaConverter,
  schema_graph::SchemaGraph,
  utils::doc_comment_lines,
};
use crate::{
  generator::ast::StructKind,
  reserved::{to_rust_field_name, to_rust_type_name},
};

const QUERY_COMPONENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');
const SUCCESS_RESPONSE_PREFIX: char = '2';
const REQUEST_SUFFIX: &str = "Request";
const REQUEST_BODY_SUFFIX: &str = "RequestBody";
const BODY_FIELD_NAME: &str = "body";

#[derive(Debug, Clone)]
struct QueryParamMapping {
  rust_field: String,
  original_name: String,
  explode: bool,
  optional: bool,
  is_array: bool,
}

#[derive(Debug, Clone)]
struct PathParamMapping {
  rust_field: String,
  original_name: String,
}

struct WarningCollector {
  warnings: Vec<String>,
}

impl WarningCollector {
  fn new() -> Self {
    Self { warnings: Vec::new() }
  }

  fn add(&mut self, message: String) {
    self.warnings.push(message);
  }

  fn into_warnings(self) -> Vec<String> {
    self.warnings
  }
}

struct ParameterCollector<'a> {
  spec: &'a Spec,
}

impl<'a> ParameterCollector<'a> {
  fn new(spec: &'a Spec) -> Self {
    Self { spec }
  }

  fn collect_parameters(&self, path: &str, operation: &Operation) -> Vec<Parameter> {
    let mut params = self.collect_path_item_parameters(path);
    self.merge_operation_parameters(&mut params, operation);
    params
  }

  fn collect_path_item_parameters(&self, path: &str) -> Vec<Parameter> {
    let mut params = Vec::new();

    if let Some(ref paths) = self.spec.paths
      && let Some(path_item) = paths.get(path)
    {
      for param_ref in &path_item.parameters {
        if let Ok(param) = param_ref.resolve(self.spec) {
          params.push(param);
        }
      }
    }

    params
  }

  fn merge_operation_parameters(&self, params: &mut Vec<Parameter>, operation: &Operation) {
    for param_ref in &operation.parameters {
      if let Ok(param) = param_ref.resolve(self.spec) {
        params.retain(|existing| !(existing.location == param.location && existing.name == param.name));
        params.push(param);
      }
    }
  }
}

type TypeValidation = (TypeRef, Vec<String>, Option<String>, Option<serde_json::Value>);

struct ParameterConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> ParameterConverter<'a> {
  fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  async fn convert_parameter(&self, param: &Parameter) -> anyhow::Result<FieldDef> {
    let (rust_type, validation_attrs, regex_validation, default_value) = self.extract_type_and_validation(param)?;
    let is_required = param.required.unwrap_or(false);
    let docs = self.build_parameter_docs(param).await;

    Ok(FieldDef {
      name: to_rust_field_name(&param.name),
      docs,
      rust_type: if is_required {
        rust_type
      } else {
        rust_type.with_option()
      },
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs,
      regex_validation,
      default_value,
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    })
  }

  fn extract_type_and_validation(&self, param: &Parameter) -> anyhow::Result<TypeValidation> {
    let Some(ref schema_ref) = param.schema else {
      return Ok((TypeRef::new("String"), vec![], None, None));
    };

    let Ok(schema) = schema_ref.resolve(self.spec) else {
      return Ok((TypeRef::new("String"), vec![], None, None));
    };

    let type_ref = self.schema_converter.schema_to_type_ref(&schema)?;
    let is_required = param.required.unwrap_or(false);
    let validation = SchemaConverter::extract_validation_attrs(&param.name, is_required, &schema);
    let regex_validation = SchemaConverter::extract_validation_pattern(&param.name, &schema).cloned();
    let default = SchemaConverter::extract_default_value(&schema);

    Ok((type_ref, validation, regex_validation, default))
  }

  async fn build_parameter_docs(&self, param: &Parameter) -> Vec<String> {
    let mut docs = Vec::new();

    if let Some(ref desc) = param.description {
      docs.extend(doc_comment_lines(desc).await);
    }

    let location_hint = match param.location {
      ParameterIn::Path => "Path",
      ParameterIn::Query => "Query",
      ParameterIn::Header => "Header",
      ParameterIn::Cookie => "Cookie",
    };

    docs.push("/// ## Schema".to_string());
    docs.push(format!("/// - Location: {location_hint}"));

    docs
  }
}

struct RequestBodyBuilder<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> RequestBodyBuilder<'a> {
  fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  async fn prepare_request_body(
    &self,
    base_name: &str,
    operation: &Operation,
  ) -> anyhow::Result<(Option<TypeRef>, Vec<RustType>, Vec<String>)> {
    let mut generated_types = Vec::new();
    let mut request_usage = Vec::new();

    let Some(body_ref) = operation.request_body.as_ref() else {
      return Ok((None, generated_types, request_usage));
    };

    let body = body_ref.resolve(self.spec)?;
    let Some((_content_type, media_type)) = body.content.iter().next() else {
      return Ok((None, generated_types, request_usage));
    };

    let Some(schema_or_ref) = media_type.schema.as_ref() else {
      return Ok((None, generated_types, request_usage));
    };

    let raw_body_type_name = format!("{base_name}{REQUEST_BODY_SUFFIX}");
    let resolved_type = self
      .process_schema(
        schema_or_ref,
        &raw_body_type_name,
        body.description.as_ref(),
        &mut generated_types,
        &mut request_usage,
      )
      .await?;

    Ok((resolved_type, generated_types, request_usage))
  }

  async fn process_schema(
    &self,
    schema_or_ref: &ObjectOrReference<oas3::spec::ObjectSchema>,
    type_name: &str,
    description: Option<&String>,
    generated_types: &mut Vec<RustType>,
    request_usage: &mut Vec<String>,
  ) -> anyhow::Result<Option<TypeRef>> {
    let rust_type_name = to_rust_type_name(type_name);

    match schema_or_ref {
      ObjectOrReference::Object(inline_schema) => {
        self
          .process_inline_schema(
            inline_schema,
            type_name,
            &rust_type_name,
            generated_types,
            request_usage,
          )
          .await
      }
      ObjectOrReference::Ref { ref_path, .. } => {
        self
          .process_ref_schema(ref_path, description, &rust_type_name, generated_types, request_usage)
          .await
      }
    }
  }

  async fn process_inline_schema(
    &self,
    inline_schema: &oas3::spec::ObjectSchema,
    raw_name: &str,
    rust_name: &str,
    generated_types: &mut Vec<RustType>,
    request_usage: &mut Vec<String>,
  ) -> anyhow::Result<Option<TypeRef>> {
    if inline_schema.properties.is_empty() {
      return Ok(None);
    }

    let (body_struct, mut inline_types) = self
      .schema_converter
      .convert_struct(raw_name, inline_schema, Some(StructKind::RequestBody))
      .await?;

    generated_types.append(&mut inline_types);
    generated_types.push(body_struct);
    request_usage.push(rust_name.to_string());

    Ok(Some(TypeRef::new(rust_name.to_string())))
  }

  async fn process_ref_schema(
    &self,
    ref_path: &str,
    description: Option<&String>,
    rust_name: &str,
    generated_types: &mut Vec<RustType>,
    request_usage: &mut Vec<String>,
  ) -> anyhow::Result<Option<TypeRef>> {
    let target_name = SchemaGraph::extract_ref_name(ref_path)
      .or_else(|| ref_path.rsplit('/').next().map(std::string::ToString::to_string));

    let Some(target) = target_name else {
      return Ok(None);
    };

    let docs = if let Some(d) = description {
      doc_comment_lines(d).await
    } else {
      vec![]
    };

    let target_rust_name = to_rust_type_name(&target);
    let alias = TypeAliasDef {
      name: rust_name.to_string(),
      docs,
      target: TypeRef::new(target_rust_name.clone()),
    };

    generated_types.push(RustType::TypeAlias(alias));
    request_usage.push(target_rust_name);
    request_usage.push(rust_name.to_string());

    Ok(Some(TypeRef::new(rust_name.to_string())))
  }
}

struct PathRenderer;

impl PathRenderer {
  fn build_render_path_method(
    path: &str,
    path_params: &[PathParamMapping],
    query_params: &[QueryParamMapping],
  ) -> StructMethod {
    let docs = vec!["/// Render the request path with percent-encoded parameters.".to_string()];

    let segments = if path_params.is_empty() {
      vec![PathSegment::Literal(path.to_string())]
    } else {
      Self::parse_path_segments(path, path_params)
    };

    let query_parameters = Self::build_query_parameters(query_params);

    StructMethod {
      name: "render_path".to_string(),
      docs,
      kind: StructMethodKind::RenderPath {
        segments,
        query_params: query_parameters,
      },
      attrs: vec!["must_use".to_string()],
    }
  }

  fn parse_path_segments(path: &str, path_params: &[PathParamMapping]) -> Vec<PathSegment> {
    let param_map: HashMap<&str, &str> = path_params
      .iter()
      .map(|p| (p.original_name.as_str(), p.rust_field.as_str()))
      .collect();

    let mut segments = Vec::new();
    let mut cursor = 0;

    while let Some(open_rel) = path[cursor..].find('{') {
      let open = cursor + open_rel;

      if open > cursor {
        segments.push(PathSegment::Literal(path[cursor..open].to_string()));
      }

      let after_open = open + 1;
      if let Some(close_rel) = path[after_open..].find('}') {
        let close = after_open + close_rel;
        let placeholder = &path[after_open..close];

        if let Some(rust_name) = param_map.get(placeholder) {
          segments.push(PathSegment::Parameter {
            field: (*rust_name).to_string(),
          });
        } else {
          segments.push(PathSegment::Literal(format!("{{{placeholder}}}")));
        }

        cursor = close + 1;
      } else {
        segments.push(PathSegment::Literal(path[open..].to_string()));
        break;
      }
    }

    if cursor < path.len() {
      segments.push(PathSegment::Literal(path[cursor..].to_string()));
    }

    segments
  }

  fn build_query_parameters(query_params: &[QueryParamMapping]) -> Vec<QueryParameter> {
    query_params
      .iter()
      .map(|mapping| QueryParameter {
        field: mapping.rust_field.clone(),
        encoded_name: Self::encode_query_name(&mapping.original_name),
        explode: mapping.explode,
        optional: mapping.optional,
        is_array: mapping.is_array,
      })
      .collect()
  }

  fn encode_query_name(name: &str) -> String {
    utf8_percent_encode(name, QUERY_COMPONENT_ENCODE_SET).to_string()
  }

  fn query_param_explode(param: &Parameter) -> bool {
    param
      .explode
      .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)))
  }
}

struct RequestStructBuilder<'a> {
  parameter_converter: ParameterConverter<'a>,
  parameter_collector: ParameterCollector<'a>,
  spec: &'a Spec,
  warnings: WarningCollector,
}

impl<'a> RequestStructBuilder<'a> {
  fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self {
      parameter_converter: ParameterConverter::new(schema_converter, spec),
      parameter_collector: ParameterCollector::new(spec),
      spec,
      warnings: WarningCollector::new(),
    }
  }

  async fn build(
    mut self,
    name: &str,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
  ) -> anyhow::Result<(StructDef, Vec<String>)> {
    let params = self.parameter_collector.collect_parameters(path, operation);

    let mut fields = Vec::new();
    let mut path_param_mappings = Vec::new();
    let mut query_param_mappings = Vec::new();

    for param in params {
      let field = self.parameter_converter.convert_parameter(&param).await?;
      self.process_parameter(&param, &field, &mut path_param_mappings, &mut query_param_mappings);
      fields.push(field);
    }

    self
      .add_request_body_fields(&mut fields, path, operation, body_type)
      .await?;

    let struct_def = self
      .build_struct_def(
        name,
        operation,
        fields,
        path,
        &path_param_mappings,
        &query_param_mappings,
      )
      .await;

    Ok((struct_def, self.warnings.into_warnings()))
  }

  fn process_parameter(
    &mut self,
    param: &Parameter,
    field: &FieldDef,
    path_params: &mut Vec<PathParamMapping>,
    query_params: &mut Vec<QueryParamMapping>,
  ) {
    match param.location {
      ParameterIn::Path => {
        if !param.required.unwrap_or(false) {
          self
            .warnings
            .add(format!("path parameter '{}' is optional", param.name));
        }
        path_params.push(PathParamMapping {
          rust_field: field.name.clone(),
          original_name: param.name.clone(),
        });
      }
      ParameterIn::Query => {
        if param.required.unwrap_or(false) {
          self
            .warnings
            .add(format!("query parameter '{}' is required", param.name));
        }
        query_params.push(QueryParamMapping {
          rust_field: field.name.clone(),
          original_name: param.name.clone(),
          explode: PathRenderer::query_param_explode(param),
          optional: field.rust_type.nullable,
          is_array: field.rust_type.is_array,
        });
      }
      ParameterIn::Header | ParameterIn::Cookie => {}
    }
  }

  async fn add_request_body_fields(
    &mut self,
    fields: &mut Vec<FieldDef>,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
  ) -> anyhow::Result<()> {
    let request_body_required = self.is_request_body_required(path, operation);

    if !request_body_required {
      return Ok(());
    }

    let Ok(Some(body)) = operation.request_body(self.spec) else {
      return Ok(());
    };

    let is_required = body.required.as_ref().unwrap_or(&false);

    for (body_count, (content_type, media_type)) in body.content.into_iter().enumerate() {
      if let Ok(Some(schema_ref)) = media_type.schema(self.spec) {
        let field = self
          .create_body_field(
            &schema_ref,
            body_type.as_ref(),
            *is_required,
            body.description.as_ref(),
            &content_type,
            body_count,
          )
          .await?;

        fields.push(field);
      }
    }

    Ok(())
  }

  async fn create_body_field(
    &mut self,
    schema_ref: &oas3::spec::ObjectSchema,
    body_type: Option<&TypeRef>,
    is_required: bool,
    description: Option<&String>,
    content_type: &str,
    body_count: usize,
  ) -> anyhow::Result<FieldDef> {
    let type_ref = if let Some(override_type) = body_type {
      override_type.clone()
    } else {
      self
        .parameter_converter
        .schema_converter
        .schema_to_type_ref(schema_ref)?
    };

    if schema_ref.properties.is_empty() && is_required && body_type.is_none() {
      self
        .warnings
        .add("required request body schema has no properties".to_string());
    }

    let validation_attrs = SchemaConverter::extract_validation_attrs("body", is_required, schema_ref);
    let regex_validation = SchemaConverter::extract_validation_pattern("body", schema_ref).cloned();
    let default_value = SchemaConverter::extract_default_value(schema_ref);

    let mut docs = if let Some(d) = description {
      doc_comment_lines(d).await
    } else {
      vec![]
    };

    docs.push("/// ## Schema".to_string());
    docs.push(format!("/// - Required: `{}`", if is_required { "yes" } else { "no" }));
    docs.push(format!("/// - Content-Type: `{content_type}`"));

    let name = if body_count > 0 {
      format!("{BODY_FIELD_NAME}_{body_count}")
    } else {
      BODY_FIELD_NAME.to_string()
    };

    Ok(FieldDef {
      name,
      docs,
      rust_type: if is_required { type_ref } else { type_ref.with_option() },
      serde_attrs: vec![],
      extra_attrs: vec![],
      validation_attrs,
      regex_validation,
      default_value,
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    })
  }

  fn is_request_body_required(&self, path: &str, operation: &Operation) -> bool {
    if let Some(ref paths) = self.spec.paths
      && let Some(item) = paths.get(path)
      && let Some((method, _)) = item.methods().into_iter().find(|m| m.1 == operation)
    {
      matches!(method, Method::POST | Method::PATCH | Method::PUT)
    } else {
      false
    }
  }

  async fn build_struct_def(
    &self,
    name: &str,
    operation: &Operation,
    fields: Vec<FieldDef>,
    path: &str,
    path_params: &[PathParamMapping],
    query_params: &[QueryParamMapping],
  ) -> StructDef {
    let docs = if let Some(d) = operation.description.as_ref().or(operation.summary.as_ref()) {
      doc_comment_lines(d).await
    } else {
      vec![]
    };

    let methods = vec![PathRenderer::build_render_path_method(path, path_params, query_params)];

    StructDef {
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
    }
  }
}

pub(crate) struct OperationConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> OperationConverter<'a> {
  pub(crate) fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  pub(crate) async fn convert_operation(
    &self,
    operation_id: &str,
    method: &str,
    path: &str,
    operation: &Operation,
  ) -> anyhow::Result<(Vec<RustType>, OperationInfo)> {
    let base_name = to_rust_type_name(operation_id);

    let body_builder = RequestBodyBuilder::new(self.schema_converter, self.spec);
    let (request_body_type, request_body_defs, request_body_usage) =
      body_builder.prepare_request_body(&base_name, operation).await?;

    let mut types = request_body_defs;
    let has_parameters = self.operation_has_parameters(path, operation);

    let mut operation_warnings = Vec::new();

    let request_type_name = if has_parameters || operation.request_body.is_some() {
      let request_name = format!("{base_name}{REQUEST_SUFFIX}");
      let (request_struct, warnings) = self
        .create_request_struct(&request_name, path, operation, request_body_type)
        .await?;

      operation_warnings.extend(warnings);

      types.push(RustType::Struct(request_struct));
      Some(request_name)
    } else {
      None
    };

    let response_type_name = self.extract_response_type_name(operation);

    let op_info = OperationInfo {
      operation_id: operation.operation_id.clone().unwrap_or_else(|| base_name.clone()),
      method: method.to_string(),
      path: path.to_string(),
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_type_name,
      request_body_types: request_body_usage,
      warnings: operation_warnings,
    };

    Ok((types, op_info))
  }

  fn operation_has_parameters(&self, path: &str, operation: &Operation) -> bool {
    if !operation.parameters.is_empty() {
      return true;
    }

    if let Some(ref paths) = self.spec.paths
      && let Some(path_item) = paths.get(path)
      && !path_item.parameters.is_empty()
    {
      return true;
    }

    false
  }

  async fn create_request_struct(
    &self,
    name: &str,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
  ) -> anyhow::Result<(StructDef, Vec<String>)> {
    let builder = RequestStructBuilder::new(self.schema_converter, self.spec);
    builder.build(name, path, operation, body_type).await
  }

  fn extract_response_type_name(&self, operation: &Operation) -> Option<String> {
    let responses = operation.responses.as_ref()?;

    responses
      .iter()
      .find(|(code, _)| code.starts_with(SUCCESS_RESPONSE_PREFIX))
      .or_else(|| responses.iter().next())
      .and_then(|(_, response_ref)| {
        response_ref
          .resolve(self.spec)
          .ok()
          .and_then(|response| Self::extract_response_schema_name(&response))
          .map(|name| to_rust_type_name(&name))
      })
  }

  fn extract_response_schema_name(response: &oas3::spec::Response) -> Option<String> {
    response.content.iter().next().and_then(|(_, media_type)| {
      media_type.schema.as_ref().and_then(|schema_ref| {
        if let ObjectOrReference::Ref { ref_path, .. } = schema_ref {
          SchemaGraph::extract_ref_name(ref_path)
        } else {
          None
        }
      })
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_warning_collector() {
    let mut collector = WarningCollector::new();
    collector.add("warning 1".to_string());
    collector.add("warning 2".to_string());

    let warnings = collector.into_warnings();
    assert_eq!(warnings.len(), 2);
    assert_eq!(warnings[0], "warning 1");
    assert_eq!(warnings[1], "warning 2");
  }

  #[test]
  fn test_path_renderer_simple_path() {
    let segments = PathRenderer::parse_path_segments("/api/users", &[]);
    assert_eq!(segments.len(), 1);
    assert!(matches!(segments[0], PathSegment::Literal(_)));
  }

  #[test]
  fn test_path_renderer_with_params() {
    let mappings = vec![PathParamMapping {
      rust_field: "user_id".to_string(),
      original_name: "userId".to_string(),
    }];

    let segments = PathRenderer::parse_path_segments("/api/users/{userId}", &mappings);
    assert_eq!(segments.len(), 2);
    assert!(matches!(segments[0], PathSegment::Literal(_)));
    assert!(matches!(segments[1], PathSegment::Parameter { .. }));
  }

  #[test]
  fn test_path_renderer_multiple_params() {
    let mappings = vec![
      PathParamMapping {
        rust_field: "org_id".to_string(),
        original_name: "orgId".to_string(),
      },
      PathParamMapping {
        rust_field: "user_id".to_string(),
        original_name: "userId".to_string(),
      },
    ];

    let segments = PathRenderer::parse_path_segments("/orgs/{orgId}/users/{userId}", &mappings);
    assert_eq!(segments.len(), 4);
  }

  #[test]
  fn test_path_renderer_encode_query_name() {
    let encoded = PathRenderer::encode_query_name("hello world");
    assert!(encoded.contains("%20"));

    let encoded = PathRenderer::encode_query_name("hello-world");
    assert_eq!(encoded, "hello-world");
  }

  #[test]
  fn test_query_param_explode_default() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      description: None,
      required: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: None,
      style: None,
      schema: None,
      content: Default::default(),
      example: None,
      examples: Default::default(),
      extensions: Default::default(),
    };

    assert!(PathRenderer::query_param_explode(&param));
  }

  #[test]
  fn test_query_param_explode_explicit() {
    let param = Parameter {
      name: "test".to_string(),
      location: ParameterIn::Query,
      description: None,
      required: None,
      deprecated: None,
      allow_empty_value: None,
      allow_reserved: None,
      explode: Some(false),
      style: None,
      schema: None,
      content: Default::default(),
      example: None,
      examples: Default::default(),
      extensions: Default::default(),
    };

    assert!(!PathRenderer::query_param_explode(&param));
  }

  #[test]
  fn test_constants() {
    assert_eq!(REQUEST_SUFFIX, "Request");
    assert_eq!(REQUEST_BODY_SUFFIX, "RequestBody");
    assert_eq!(BODY_FIELD_NAME, "body");
    assert_eq!(SUCCESS_RESPONSE_PREFIX, '2');
  }
}
