use std::collections::HashMap;

use http::Method;
use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn, ParameterStyle},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use super::{ast::*, schema_converter::SchemaConverter, schema_graph::SchemaGraph, utils::doc_comment_lines};
use crate::reserved::{to_rust_field_name, to_rust_type_name};

const QUERY_COMPONENT_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Debug, Clone)]
struct QueryParamMapping {
  rust_field: String,
  original_name: String,
  explode: bool,
  optional: bool,
  is_array: bool,
}

/// Converter for OpenAPI operations to Rust request/response types
pub(crate) struct OperationConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> OperationConverter<'a> {
  pub(crate) fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  /// Convert an operation to request and response types
  pub(crate) fn convert_operation(
    &self,
    operation_id: &str,
    method: &str,
    path: &str,
    operation: &Operation,
  ) -> anyhow::Result<(Vec<RustType>, OperationInfo)> {
    let mut types = Vec::new();
    let base_name = to_rust_type_name(operation_id);

    let (request_body_type, mut request_body_defs, request_body_usage) =
      self.prepare_request_body(&base_name, operation)?;
    types.append(&mut request_body_defs);

    let has_parameters = self.operation_has_parameters(path, operation);

    let request_type_name = if has_parameters || operation.request_body.is_some() {
      let request_name = format!("{}Request", base_name);
      let request_struct = self.create_request_struct(&request_name, path, operation, request_body_type)?;
      types.push(RustType::Struct(request_struct));
      Some(request_name)
    } else {
      None
    };

    // Extract primary response type (typically 200/201 response)
    // Don't generate response enums - let HTTP clients use http::StatusCode
    let response_type_name = if let Some(ref responses) = operation.responses {
      // Look for successful response (200, 201, etc.)
      responses
        .iter()
        .find(|(code, _)| code.starts_with('2'))
        .or_else(|| responses.iter().next())
        .and_then(|(_, response_ref)| {
          if let Ok(response) = response_ref.resolve(self.spec) {
            self.extract_response_schema_name(&response)
          } else {
            None
          }
        })
        .map(|name| to_rust_type_name(&name))
    } else {
      None
    };

    let op_info = OperationInfo {
      operation_id: operation.operation_id.clone().unwrap_or_else(|| base_name.clone()),
      method: method.to_string(),
      path: path.to_string(),
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_type_name,
      request_body_types: request_body_usage,
    };

    Ok((types, op_info))
  }

  /// Return `Method` of the operation built from the `path`
  #[inline]
  fn operation_method(&self, path: &str, operation: &Operation) -> Option<Method> {
    if let Some(ref paths) = self.spec.paths
      && let Some(item) = paths.get(path)
      && let Some((method, _)) = item.methods().into_iter().find(|m| m.1 == operation)
    {
      Some(method)
    } else {
      None
    }
  }

  #[inline]
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

  fn prepare_request_body(
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

    let raw_body_type_name = format!("{base_name}RequestBody");
    let rust_body_type_name = to_rust_type_name(&raw_body_type_name);
    let mut resolved_type: Option<TypeRef> = None;

    match schema_or_ref {
      ObjectOrReference::Object(inline_schema) => {
        if !inline_schema.properties.is_empty() {
          let (body_struct, mut inline_types) = self
            .schema_converter
            .convert_struct(&raw_body_type_name, inline_schema)?;
          generated_types.append(&mut inline_types);
          generated_types.push(body_struct);
          resolved_type = Some(TypeRef::new(rust_body_type_name.clone()));
          request_usage.push(rust_body_type_name.clone());
        }
      }
      ObjectOrReference::Ref { ref_path, .. } => {
        let target_name = SchemaGraph::extract_ref_name(ref_path)
          .or_else(|| ref_path.rsplit('/').next().map(|segment| segment.to_string()));

        if let Some(target) = target_name {
          let alias = TypeAliasDef {
            name: rust_body_type_name.clone(),
            docs: body
              .description
              .as_ref()
              .map(|d| doc_comment_lines(d))
              .unwrap_or_default(),
            target: TypeRef::new(to_rust_type_name(&target)),
          };
          generated_types.push(RustType::TypeAlias(alias));
          resolved_type = Some(TypeRef::new(rust_body_type_name.clone()));
          request_usage.push(to_rust_type_name(&target));
          request_usage.push(rust_body_type_name.clone());
        }
      }
    }

    if resolved_type.is_none()
      && let Ok(resolved_schema) = schema_or_ref.resolve(self.spec)
    {
      resolved_type = Some(self.schema_converter.schema_to_type_ref(&resolved_schema)?);
    }

    Ok((resolved_type, generated_types, request_usage))
  }

  /// Create a request struct from operation parameters and body
  fn create_request_struct(
    &self,
    name: &str,
    path: &str,
    operation: &Operation,
    body_type: Option<TypeRef>,
  ) -> anyhow::Result<StructDef> {
    let mut fields = Vec::new();
    let mut path_param_mappings: Vec<(String, String)> = Vec::new();
    let mut query_param_mappings: Vec<QueryParamMapping> = Vec::new();
    let mut params: Vec<Parameter> = Vec::new();

    if let Some(ref paths) = self.spec.paths
      && let Some(path_item) = paths.get(path)
    {
      for param_ref in &path_item.parameters {
        if let Ok(param) = param_ref.resolve(self.spec)
          && !params
            .iter()
            .any(|existing| existing.location == param.location && existing.name == param.name)
        {
          params.push(param);
        }
      }
    }

    for param_ref in &operation.parameters {
      if let Ok(param) = param_ref.resolve(self.spec) {
        params.retain(|existing| !(existing.location == param.location && existing.name == param.name));
        params.push(param);
      }
    }

    for param in params {
      let field = self.convert_parameter(&param)?;
      let optional = field.rust_type.nullable;
      let is_array = field.rust_type.is_array;

      match param.location {
        ParameterIn::Path => {
          if !param.required.unwrap_or(false) {
            eprintln!(
              "[{}] warning: path parameter '{}' is optional.",
              operation.operation_id.as_deref().unwrap_or("unknown"),
              param.name,
            );
          }
          path_param_mappings.push((field.name.clone(), param.name.clone()));
        }
        ParameterIn::Query => {
          if param.required.unwrap_or(false) {
            eprintln!(
              "[{}] warning: query parameter '{}' is required.",
              operation.operation_id.as_deref().unwrap_or("unknown"),
              param.name,
            );
          }
          query_param_mappings.push(QueryParamMapping {
            rust_field: field.name.clone(),
            original_name: param.name.clone(),
            explode: Self::query_param_explode(&param),
            optional,
            is_array,
          });
        }
        ParameterIn::Header => {
          // TODO: handle header parameters if needed
        }
        ParameterIn::Cookie => {
          // TODO: handle cookie parameters if needed
          eprintln!(
            "[{}] warning: cookie parameter '{}' is not supported",
            operation.operation_id.as_deref().unwrap_or("unknown"),
            param.name,
          );
        }
      }

      fields.push(field);
    }

    // Check if body is expected based on HTTP method type
    let request_body_required = self
      .operation_method(path, operation)
      .map(|m| matches!(m, Method::POST | Method::PATCH | Method::PUT))
      .unwrap_or(false);

    if request_body_required && let Ok(Some(body)) = operation.request_body(self.spec) {
      let is_required = body.required.as_ref().unwrap_or(&false);

      for (body_count, (content_type, media_type)) in body.content.into_iter().enumerate() {
        if let Ok(Some(schema_ref)) = media_type.schema(self.spec) {
          let type_ref = if let Some(ref override_type) = body_type {
            override_type.clone()
          } else {
            self.schema_converter.schema_to_type_ref(&schema_ref)?
          };

          if schema_ref.properties.is_empty() && *is_required && body_type.is_none() {
            eprintln!(
              "[{}] error: required request body schema has no properties.",
              operation.operation_id.as_deref().unwrap_or("unknown"),
            );
          }

          let validation_attrs = self
            .schema_converter
            .extract_validation_attrs(name, *is_required, &schema_ref);
          let regex_validation = self
            .schema_converter
            .extract_validation_pattern(name, &schema_ref)
            .cloned();
          let default_value = self.schema_converter.extract_default_value(&schema_ref);

          let serde_attrs = vec![];

          let mut docs = body
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default();

          docs.push("/// ## Schema".to_string());
          docs.push(format!("/// - Required: `{}`", if *is_required { "yes" } else { "no" }));
          docs.push(format!("/// - Content-Type: `{content_type}`"));

          let name = if body_count > 1 {
            format!("body_{body_count}")
          } else {
            "body".to_string()
          };

          fields.push(FieldDef {
            name,
            docs,
            rust_type: if *is_required { type_ref } else { type_ref.with_option() },
            serde_attrs,
            validation_attrs,
            regex_validation,
            default_value,
            read_only: false,
            write_only: false,
            deprecated: false,
            multiple_of: None,
          });
        }
      }
    }

    let docs = operation
      .description
      .as_ref()
      .or(operation.summary.as_ref())
      .map(|d| doc_comment_lines(d))
      .unwrap_or_default();

    // Individual rename attributes are more explicit and handle all edge cases correctly
    let mut serde_attrs = vec![];

    // Only add serde(default) at struct level if ALL fields have defaults or are Option/Vec
    let all_fields_defaultable = fields.iter().all(|f| {
      f.default_value.is_some()
        || f.rust_type.nullable
        || f.rust_type.is_array
        || matches!(
          f.rust_type.base_type.as_str(),
          "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "serde_json::Value"
        )
    });

    let outer_attrs = SchemaConverter::container_outer_attrs(&fields);
    if fields.iter().any(|f| f.default_value.is_some()) && all_fields_defaultable {
      serde_attrs.push("default".to_string());
    }

    let derives = vec![
      "Clone".into(),
      "Debug".into(),
      "Serialize".into(),
      "Deserialize".into(),
      "PartialEq".into(),
      "validator::Validate".into(),
      "oas3_gen_support::Default".into(),
    ];

    let methods = vec![Self::build_render_path_method(
      path,
      &path_param_mappings,
      &query_param_mappings,
    )];

    Ok(StructDef {
      name: to_rust_type_name(name),
      docs,
      fields,
      derives,
      serde_attrs,
      outer_attrs,
      methods,
    })
  }

  fn build_render_path_method(
    path: &str,
    path_params: &[(String, String)],
    query_params: &[QueryParamMapping],
  ) -> StructMethod {
    let docs = vec!["/// Render the request path with percent-encoded parameters.".to_string()];

    if path_params.is_empty() {
      let segments = vec![PathSegment::Literal(path.to_string())];
      return StructMethod {
        name: "render_path".to_string(),
        docs,
        kind: StructMethodKind::RenderPath {
          segments,
          query_params: query_params
            .iter()
            .map(|mapping| QueryParameter {
              field: mapping.rust_field.clone(),
              encoded_name: Self::encode_query_name(&mapping.original_name),
              explode: mapping.explode,
              optional: mapping.optional,
              is_array: mapping.is_array,
            })
            .collect(),
        },
      };
    }

    let mut param_map: HashMap<&str, &str> = HashMap::new();
    for (rust_name, original_name) in path_params {
      param_map.insert(original_name.as_str(), rust_name.as_str());
    }

    let mut segments: Vec<PathSegment> = Vec::new();
    let mut cursor = 0usize;
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
          segments.push(PathSegment::Literal(format!("{{{}}}", placeholder)));
        }
        cursor = close + 1;
      } else {
        segments.push(PathSegment::Literal(path[open..].to_string()));
        cursor = path.len();
        break;
      }
    }

    if cursor < path.len() {
      segments.push(PathSegment::Literal(path[cursor..].to_string()));
    }

    let query_params = query_params
      .iter()
      .map(|mapping| QueryParameter {
        field: mapping.rust_field.clone(),
        encoded_name: Self::encode_query_name(&mapping.original_name),
        explode: mapping.explode,
        optional: mapping.optional,
        is_array: mapping.is_array,
      })
      .collect();

    StructMethod {
      name: "render_path".to_string(),
      docs,
      kind: StructMethodKind::RenderPath { segments, query_params },
    }
  }

  fn query_param_explode(param: &Parameter) -> bool {
    if let Some(explode) = param.explode {
      explode
    } else {
      matches!(param.style, None | Some(ParameterStyle::Form))
    }
  }

  fn encode_query_name(name: &str) -> String {
    utf8_percent_encode(name, QUERY_COMPONENT_ENCODE_SET).to_string()
  }

  /// Convert a parameter to a field definition
  fn convert_parameter(&self, param: &Parameter) -> anyhow::Result<FieldDef> {
    let (rust_type, validation_attrs, regex_validation, default_value) = if let Some(ref schema_ref) = param.schema {
      if let Ok(schema) = schema_ref.resolve(self.spec) {
        let type_ref = self.schema_converter.schema_to_type_ref(&schema)?;
        let is_required = param.required.unwrap_or(false);
        let validation = self
          .schema_converter
          .extract_validation_attrs(&param.name, is_required, &schema);
        let regex_validation = self.schema_converter.extract_validation_pattern(&param.name, &schema);
        let default = self.schema_converter.extract_default_value(&schema);
        (type_ref, validation, regex_validation.cloned(), default)
      } else {
        (TypeRef::new("String"), vec![], None, None)
      }
    } else {
      (TypeRef::new("String"), vec![], None, None)
    };

    let is_required = param.required.unwrap_or(false);

    let mut serde_attrs = vec![];
    // Add rename if the Rust field name differs from the original parameter name
    let rust_param_name = to_rust_field_name(&param.name);
    if rust_param_name != param.name.as_str() {
      serde_attrs.push(format!("rename = \"{}\"", param.name));
    }

    // Add location hint as a comment in docs
    let location_hint = match param.location {
      ParameterIn::Path => "Path",
      ParameterIn::Query => "Query",
      ParameterIn::Header => "Header",
      ParameterIn::Cookie => "Cookie",
    };

    let mut docs = vec![];
    if let Some(ref desc) = param.description {
      docs.extend(doc_comment_lines(desc));
    }
    docs.push("/// ## Schema".to_string());
    docs.push(format!("/// - Location: {}", location_hint));

    Ok(FieldDef {
      name: to_rust_field_name(&param.name),
      docs,
      rust_type: if is_required {
        rust_type
      } else {
        rust_type.with_option()
      },
      serde_attrs,
      validation_attrs,
      regex_validation,
      default_value,
      read_only: false,
      write_only: false,
      deprecated: false,
      multiple_of: None,
    })
  }

  /// Extract schema name from a response (helper)
  fn extract_response_schema_name(&self, response: &oas3::spec::Response) -> Option<String> {
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
