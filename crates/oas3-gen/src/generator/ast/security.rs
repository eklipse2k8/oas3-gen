use std::collections::HashMap;

use itertools::Itertools;

use super::{Documentation, FieldDef, FieldNameToken, TypeRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum ApiKeyLocation {
  Header,
  Query,
  Cookie,
}

impl ApiKeyLocation {
  #[must_use]
  pub fn describe(self) -> &'static str {
    match self {
      Self::Header => "header",
      Self::Query => "query parameter",
      Self::Cookie => "cookie",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiKeyScheme {
  pub scheme_name: String,
  pub field: FieldNameToken,
  pub location: ApiKeyLocation,
  pub parameter_name: String,
  pub description: Option<String>,
}

impl ApiKeyScheme {
  /// Where the key travels, e.g. ``the `X-Api-Key` header``.
  #[must_use]
  pub fn whereabouts(&self) -> String {
    format!("the `{}` {}", self.parameter_name, self.location.describe())
  }

  fn docs(&self) -> Documentation {
    let origin = format!("`{}` API key, read from {}.", self.scheme_name, self.whereabouts());
    let Some(description) = &self.description else {
      return Documentation::from_lines([origin]);
    };
    let mut docs = Documentation::from_optional(Some(description));
    docs.push("");
    docs.push(origin);
    docs
  }
}

/// The `apiKey` security an operation accepts.
///
/// `requirements` are alternatives: a request is authorized when it carries every
/// scheme of at least one of them. An empty requirement allows anonymous access.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationSecurity {
  pub schemes: Vec<ApiKeyScheme>,
  pub requirements: Vec<Vec<FieldNameToken>>,
}

impl OperationSecurity {
  /// Whether any accepted key travels in `location`.
  #[must_use]
  pub fn reads(&self, location: ApiKeyLocation) -> bool {
    self.schemes.iter().any(|scheme| scheme.location == location)
  }

  /// Whether every authorized request carries `field`.
  #[must_use]
  pub fn requires(&self, field: &FieldNameToken) -> bool {
    self.requirements.iter().all(|requirement| requirement.contains(field))
  }

  /// The credentials struct fields, required only when every alternative needs the scheme.
  #[must_use]
  pub fn credential_fields(&self) -> Vec<FieldDef> {
    self
      .schemes
      .iter()
      .map(|scheme| {
        let rust_type = TypeRef::new("secrecy::SecretString");
        FieldDef::builder()
          .name(scheme.field.clone())
          .docs(scheme.docs())
          .rust_type(if self.requires(&scheme.field) {
            rust_type
          } else {
            rust_type.with_option()
          })
          .build()
      })
      .collect()
  }

  /// The alternatives a handler checks after reading the required schemes, as the
  /// optional fields each one needs.
  ///
  /// Empty when the required schemes alone satisfy some alternative, so no check is needed.
  #[must_use]
  pub fn unchecked_alternatives(&self) -> Vec<Vec<&FieldNameToken>> {
    let mut alternatives = vec![];
    for requirement in &self.requirements {
      let optional = requirement.iter().filter(|field| !self.requires(field)).collect_vec();
      if optional.is_empty() {
        return vec![];
      }
      alternatives.push(optional);
    }
    alternatives
  }

  /// Names the accepted alternatives, e.g. ``credentials for `A` or `B` `` or
  /// ``credentials for `A` and `B`, or `C` ``.
  #[must_use]
  pub fn describe_alternatives(&self) -> String {
    let names = self
      .schemes
      .iter()
      .map(|scheme| (&scheme.field, scheme.scheme_name.as_str()))
      .collect::<HashMap<_, _>>();
    let mut alternatives = vec![];
    for requirement in &self.requirements {
      let schemes = requirement
        .iter()
        .map(|field| format!("`{}`", names[field]))
        .join(" and ");
      alternatives.push(schemes);
    }
    let separator = if self.requirements.iter().any(|requirement| requirement.len() > 1) {
      ", or "
    } else {
      " or "
    };
    format!("credentials for {}", alternatives.join(separator))
  }
}
