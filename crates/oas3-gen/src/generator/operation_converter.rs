//! Operation converter for transforming OpenAPI operations to Rust request/response types
//!
//! This module handles the conversion of OpenAPI operation definitions (paths, methods)
//! into Rust struct types for requests and response metadata.

use std::{cmp::Ordering, collections::HashMap};

use oas3::{
  Spec,
  spec::{ObjectOrReference, Operation, Parameter, ParameterIn},
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::{ast::*, schema_converter::SchemaConverter, schema_graph::SchemaGraph, utils::doc_comment_lines};
use crate::reserved::{to_rust_field_name, to_rust_type_name};

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

    // Generate a base name for the operation
    let base_name = to_rust_type_name(operation_id);

    // Generate inline request body struct if needed
    if let Some(ref body_ref) = operation.request_body
      && let Ok(body) = body_ref.resolve(self.spec)
      && let Some((_content_type, media_type)) = body.content.iter().next()
      && let Some(ref schema_ref) = media_type.schema
    {
      // Check if this is an inline schema (not a $ref)
      if let ObjectOrReference::Object(inline_schema) = schema_ref
        && !inline_schema.properties.is_empty()
      {
        // Generate a struct for the inline request body
        let body_struct_name = format!("{}RequestBody", base_name);
        let (body_struct, inline_types) = self.schema_converter.convert_struct(&body_struct_name, inline_schema)?;

        // Add inline types first (if any)
        types.extend(inline_types);

        // Add the body struct
        types.push(body_struct);
      }
    }

    // Generate request type if needed
    let request_type_name = if !operation.parameters.is_empty() || operation.request_body.is_some() {
      let request_name = format!("{}Request", base_name);
      let request_struct = self.create_request_struct(&request_name, &base_name, path, operation)?;
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
    };

    Ok((types, op_info))
  }

  /// Create a request struct from operation parameters and body
  fn create_request_struct(
    &self,
    name: &str,
    base_name: &str,
    path: &str,
    operation: &Operation,
  ) -> anyhow::Result<StructDef> {
    let mut fields = Vec::new();
    let mut path_param_mappings: Vec<(String, String)> = Vec::new();

    // Process parameters
    let mut params: Vec<_> = operation
      .parameters
      .iter()
      .filter_map(|param_ref| param_ref.resolve(self.spec).ok())
      .collect();

    params.sort_by(|a, b| {
      let rank = |loc: &ParameterIn| match loc {
        ParameterIn::Path => 0u8,
        ParameterIn::Query => 1,
        ParameterIn::Header => 2,
        ParameterIn::Cookie => 3,
      };

      match rank(&a.location).cmp(&rank(&b.location)) {
        Ordering::Equal => a.name.cmp(&b.name),
        other => other,
      }
    });

    for param in params {
      let field = self.convert_parameter(&param)?;
      if matches!(param.location, ParameterIn::Path) {
        path_param_mappings.push((field.name.clone(), param.name.clone()));
      }
      fields.push(field);
    }

    // Process request body
    if let Some(ref body_ref) = operation.request_body
      && let Ok(body) = body_ref.resolve(self.spec)
    {
      // Extract schema from the first content type (usually application/json)
      if let Some((_content_type, media_type)) = body.content.iter().next()
        && let Some(ref schema_ref) = media_type.schema
      {
        // Check if this is an inline schema (not a $ref) with properties
        let body_type = if let ObjectOrReference::Object(inline_schema) = schema_ref
          && !inline_schema.properties.is_empty()
        {
          // Use the generated inline body struct (apply same transformation as convert_struct does)
          let body_struct_name = format!("{}RequestBody", base_name);
          TypeRef::new(to_rust_type_name(&body_struct_name))
        } else if let Ok(schema) = schema_ref.resolve(self.spec) {
          // Use existing logic for $ref or other schemas
          self.schema_converter.schema_to_type_ref(&schema)?
        } else {
          TypeRef::new("serde_json::Value")
        };

        let is_required = body.required.unwrap_or(false);

        // Get validation attrs from resolved schema if possible
        let (validation_attrs, regex_validation, default_value) = if let Ok(schema) = schema_ref.resolve(self.spec) {
          let validation = self
            .schema_converter
            .extract_validation_attrs(name, is_required, &schema);
          let regex = self.schema_converter.extract_validation_pattern(name, &schema).cloned();
          let default = self.schema_converter.extract_default_value(&schema);
          (validation, regex, default)
        } else {
          (vec![], None, None)
        };

        let serde_attrs = vec![];
        fields.push(FieldDef {
          name: "body".to_string(),
          docs: body
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default(),
          rust_type: if is_required {
            body_type
          } else {
            body_type.with_option()
          },
          serde_attrs,
          validation_attrs,
          regex_validation,
          default_value,
          read_only: false,
          write_only: false, // Request body fields are typically write-only, but we keep both derives for flexibility
          deprecated: false,
          multiple_of: None,
        });
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

    let methods = vec![Self::build_render_path_method(path, &path_param_mappings)];

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

  fn build_render_path_method(path: &str, path_params: &[(String, String)]) -> StructMethod {
    let docs = vec!["/// Render the request path with percent-encoded parameters.".to_string()];

    if path_params.is_empty() {
      let template = path.to_string();
      let tokens = quote! {
        pub fn render_path(&self) -> String {
          let _ = self;
          #template.to_string()
        }
      };

      return StructMethod { docs, tokens };
    }

    let mut param_map: HashMap<&str, &str> = HashMap::new();
    for (rust_name, original_name) in path_params {
      param_map.insert(original_name.as_str(), rust_name.as_str());
    }

    enum Segment {
      Literal(String),
      Parameter { rust_name: String },
    }

    let mut segments: Vec<Segment> = Vec::new();
    let mut cursor = 0usize;
    while let Some(open_rel) = path[cursor..].find('{') {
      let open = cursor + open_rel;
      if open > cursor {
        segments.push(Segment::Literal(path[cursor..open].to_string()));
      }

      let after_open = open + 1;
      if let Some(close_rel) = path[after_open..].find('}') {
        let close = after_open + close_rel;
        let placeholder = &path[after_open..close];
        if let Some(rust_name) = param_map.get(placeholder) {
          segments.push(Segment::Parameter {
            rust_name: (*rust_name).to_string(),
          });
        } else {
          segments.push(Segment::Literal(format!("{{{}}}", placeholder)));
        }
        cursor = close + 1;
      } else {
        segments.push(Segment::Literal(path[open..].to_string()));
        cursor = path.len();
        break;
      }
    }

    if cursor < path.len() {
      segments.push(Segment::Literal(path[cursor..].to_string()));
    }

    let mut segment_tokens: Vec<TokenStream> = Vec::new();
    for segment in segments {
      match segment {
        Segment::Literal(lit) => {
          if !lit.is_empty() {
            segment_tokens.push(quote! { path.push_str(#lit); });
          }
        }
        Segment::Parameter { rust_name } => {
          let ident = format_ident!("{}", rust_name);
          segment_tokens.push(quote! {
            {
              let value = self.#ident.to_string();
              write!(
                path,
                "{}",
                oas3_gen_support::utf8_percent_encode(value.as_str(), oas3_gen_support::PATH_ENCODE_SET)
              )
              .expect("failed to encode path parameter");
            }
          });
        }
      }
    }

    let tokens = quote! {
      pub fn render_path(&self) -> String {
        let mut path = String::new();
        #(#segment_tokens)*
        path
      }
    };

    StructMethod { docs, tokens }
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
      ParameterIn::Path => "Path parameter",
      ParameterIn::Query => "Query parameter",
      ParameterIn::Header => "Header parameter",
      ParameterIn::Cookie => "Cookie parameter",
    };

    let mut docs = vec![format!("/// {}", location_hint)];
    if let Some(ref desc) = param.description {
      docs.extend(doc_comment_lines(desc));
    }

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
