use std::collections::HashMap;

use oas3::spec::{Parameter, ParameterStyle};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::generator::ast::{PathSegment, QueryParameter, StructMethod, StructMethodKind};

const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Debug, Clone)]
pub(crate) struct PathParamMapping {
  pub rust_field: String,
  pub original_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryParamMapping {
  pub rust_field: String,
  pub original_name: String,
  pub explode: bool,
  pub style: Option<ParameterStyle>,
  pub optional: bool,
  pub is_array: bool,
}

pub(crate) fn build_render_path_method(
  path: &str,
  path_params: &[PathParamMapping],
  query_params: &[QueryParamMapping],
) -> StructMethod {
  let query_parameters = query_params
    .iter()
    .map(|m| QueryParameter {
      field: m.rust_field.clone(),
      encoded_name: encode_query_name(&m.original_name),
      explode: m.explode,
      optional: m.optional,
      is_array: m.is_array,
      style: m.style,
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
  let mut segments = vec![];
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

pub(crate) fn encode_query_name(name: &str) -> String {
  utf8_percent_encode(name, QUERY_ENCODE_SET).to_string()
}

pub(crate) fn query_param_explode(param: &Parameter) -> bool {
  param
    .explode
    .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)))
}
