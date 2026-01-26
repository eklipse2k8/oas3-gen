use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use strum::Display;

use super::{FieldDef, ParameterLocation, tokens::FieldNameToken};

#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum PathParseError {
  #[strum(to_string = "unclosed '{{' at position {position} in segment '{segment}'")]
  UnclosedBrace { segment: String, position: usize },
  #[strum(to_string = "empty parameter '{{}}' in segment '{segment}'")]
  EmptyParameter { segment: String },
  #[strum(to_string = "unmatched '}}' at position {position} in segment '{segment}'")]
  UnmatchedClosingBrace { segment: String, position: usize },
  #[strum(to_string = "nested '{{' at position {position} in segment '{segment}'")]
  NestedBraces { segment: String, position: usize },
}

impl std::error::Error for PathParseError {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
  Literal(String),
  Param(FieldNameToken),
  Mixed {
    format: String,
    params: Vec<FieldNameToken>,
  },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SegmentPart<'a> {
  Literal(&'a str),
  Param(&'a str),
}

impl PathSegment {
  #[cfg(test)]
  pub fn is_mixed(&self) -> bool {
    matches!(self, Self::Mixed { .. })
  }

  pub fn to_axum_segment(&self) -> String {
    match self {
      Self::Literal(lit) => lit.clone(),
      Self::Param(field) => format!("{{{}}}", field.as_str()),
      Self::Mixed { format, params } => {
        let mut result = format.clone();
        for param in params {
          if let Some(pos) = result.find("{}") {
            result.replace_range(pos..pos + 2, &format!("{{{}}}", param.as_str()));
          }
        }
        result
      }
    }
  }

  pub fn parse(segment: &str, params: &HashMap<&str, &FieldNameToken>) -> Result<Self, PathParseError> {
    let parts = Self::tokenize(segment)?;

    match parts.as_slice() {
      [] => Ok(Self::Literal(String::new())),
      [SegmentPart::Literal(lit)] => Ok(Self::Literal((*lit).to_string())),
      [SegmentPart::Param(name)] => {
        let field = params
          .get(name)
          .map_or_else(|| FieldNameToken::new(name), |f| (*f).clone());
        Ok(Self::Param(field))
      }
      _ => Ok(Self::build_mixed(segment, &parts, params)),
    }
  }

  fn tokenize(segment: &str) -> Result<Vec<SegmentPart<'_>>, PathParseError> {
    let mut parts = vec![];
    let mut rest = segment;
    let mut offset = 0;

    while !rest.is_empty() {
      let Some(open_pos) = rest.find('{') else {
        if !rest.is_empty() {
          parts.push(SegmentPart::Literal(rest));
        }
        break;
      };

      if let Some(stray_close) = rest[..open_pos].find('}') {
        return Err(PathParseError::UnmatchedClosingBrace {
          segment: segment.to_string(),
          position: offset + stray_close,
        });
      }

      if open_pos > 0 {
        parts.push(SegmentPart::Literal(&rest[..open_pos]));
      }

      let after_open = &rest[open_pos + 1..];
      let Some(close_pos) = after_open.find('}') else {
        return Err(PathParseError::UnclosedBrace {
          segment: segment.to_string(),
          position: offset + open_pos,
        });
      };

      if let Some(nested) = after_open[..close_pos].find('{') {
        return Err(PathParseError::NestedBraces {
          segment: segment.to_string(),
          position: offset + open_pos + 1 + nested,
        });
      }

      let param_name = &after_open[..close_pos];
      if param_name.is_empty() {
        return Err(PathParseError::EmptyParameter {
          segment: segment.to_string(),
        });
      }

      parts.push(SegmentPart::Param(param_name));

      let consumed = open_pos + 1 + close_pos + 1;
      offset += consumed;
      rest = &rest[consumed..];
    }

    if let Some(stray_close) = rest.find('}') {
      return Err(PathParseError::UnmatchedClosingBrace {
        segment: segment.to_string(),
        position: offset + stray_close,
      });
    }

    Ok(parts)
  }

  fn build_mixed(segment: &str, parts: &[SegmentPart<'_>], params: &HashMap<&str, &FieldNameToken>) -> Self {
    let mut format_str = String::new();
    let mut field_params = vec![];

    for part in parts {
      match part {
        SegmentPart::Literal(lit) => format_str.push_str(lit),
        SegmentPart::Param(name) => {
          let field = params
            .get(name)
            .map_or_else(|| FieldNameToken::new(name), |f| (*f).clone());
          format_str.push_str("{}");
          field_params.push(field);
        }
      }
    }

    if field_params.is_empty() {
      return Self::Literal(segment.to_string());
    }

    Self::Mixed {
      format: format_str,
      params: field_params,
    }
  }
}

impl ToTokens for PathSegment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let segment_tokens = match self {
      PathSegment::Literal(lit) => quote! { .push(#lit) },
      PathSegment::Param(field) => quote! { .push(&request.path.#field.to_string()) },
      PathSegment::Mixed { format, params } => {
        let args = params.iter().map(|f| quote! { request.path.#f });
        quote! { .push(&format!(#format, #(#args),*)) }
      }
    };
    ToTokens::to_tokens(&segment_tokens, tokens);
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ParsedPath(pub Vec<PathSegment>);

impl ParsedPath {
  pub fn parse(path: &str, parameters: &[FieldDef]) -> Result<Self, PathParseError> {
    let param_map: HashMap<&str, &FieldNameToken> = parameters
      .iter()
      .filter(|p| matches!(p.parameter_location, Some(ParameterLocation::Path)))
      .filter_map(|p| p.original_name.as_deref().map(|name| (name, &p.name)))
      .collect();

    let segments: Result<Vec<_>, _> = path
      .trim_start_matches('/')
      .split('/')
      .filter(|s| !s.is_empty())
      .map(|segment| PathSegment::parse(segment, &param_map))
      .collect();

    Ok(Self(segments?))
  }

  pub fn to_axum_path(&self) -> String {
    if self.0.is_empty() {
      return "/".to_string();
    }

    self
      .0
      .iter()
      .map(PathSegment::to_axum_segment)
      .fold(String::new(), |mut acc, seg| {
        acc.push('/');
        acc.push_str(&seg);
        acc
      })
  }

  #[cfg(test)]
  pub fn has_mixed_segments(&self) -> bool {
    self.0.iter().any(PathSegment::is_mixed)
  }

  pub fn extract_template_params(path: &str) -> impl Iterator<Item = &str> {
    TemplateParamIter::new(path)
  }
}

struct TemplateParamIter<'a> {
  rest: &'a str,
}

impl<'a> TemplateParamIter<'a> {
  fn new(path: &'a str) -> Self {
    Self { rest: path }
  }
}

impl<'a> Iterator for TemplateParamIter<'a> {
  type Item = &'a str;

  fn next(&mut self) -> Option<Self::Item> {
    let open_pos = self.rest.find('{')?;
    let after_open = &self.rest[open_pos + 1..];
    let close_pos = after_open.find('}')?;
    let param = &after_open[..close_pos];
    self.rest = &after_open[close_pos + 1..];

    if param.is_empty() { self.next() } else { Some(param) }
  }
}
