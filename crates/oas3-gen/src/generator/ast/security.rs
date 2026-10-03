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

/// How a credential travels in a request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CredentialKind {
  /// An `apiKey` scheme's key, sent in the named header, query parameter, or cookie.
  ApiKey {
    location: ApiKeyLocation,
    parameter_name: String,
  },
  /// An `http` scheme's bearer token, sent in the `Authorization` header.
  Bearer,
}

impl CredentialKind {
  /// What generated docs and messages call the credential.
  #[must_use]
  pub fn noun(&self) -> &'static str {
    match self {
      Self::ApiKey { .. } => "API key",
      Self::Bearer => "bearer token",
    }
  }
}

/// A credential an operation can accept, declared by a supported security scheme.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CredentialScheme {
  pub scheme_name: String,
  pub field: FieldNameToken,
  pub kind: CredentialKind,
  pub description: Option<String>,
}

impl CredentialScheme {
  /// Where the credential travels, e.g. ``the `X-Api-Key` header``.
  #[must_use]
  pub fn whereabouts(&self) -> String {
    match &self.kind {
      CredentialKind::ApiKey {
        location,
        parameter_name,
      } => format!("the `{parameter_name}` {}", location.describe()),
      CredentialKind::Bearer => "the `Authorization` header".to_string(),
    }
  }

  /// Where an API key travels, or `None` for a bearer token.
  #[must_use]
  pub fn api_key_location(&self) -> Option<ApiKeyLocation> {
    match self.kind {
      CredentialKind::ApiKey { location, .. } => Some(location),
      CredentialKind::Bearer => None,
    }
  }

  /// Whether the credential travels in the `Authorization` header, which a request
  /// carries once.
  #[must_use]
  pub fn uses_authorization(&self) -> bool {
    match &self.kind {
      CredentialKind::ApiKey {
        location: ApiKeyLocation::Header,
        parameter_name,
      } => parameter_name.eq_ignore_ascii_case("authorization"),
      CredentialKind::ApiKey { .. } => false,
      CredentialKind::Bearer => true,
    }
  }

  /// Whether the credential is a cookie, which the client keeps in its cookie store
  /// rather than in a field.
  #[must_use]
  pub fn is_cookie(&self) -> bool {
    self.api_key_location() == Some(ApiKeyLocation::Cookie)
  }

  fn docs(&self) -> Documentation {
    let origin = format!(
      "`{}` {}, read from {}.",
      self.scheme_name,
      self.kind.noun(),
      self.whereabouts()
    );
    let Some(description) = &self.description else {
      return Documentation::from_lines([origin]);
    };
    let mut docs = Documentation::from_optional(Some(description));
    docs.push("");
    docs.push(origin);
    docs
  }
}

/// The `apiKey` and bearer security an operation accepts.
///
/// `requirements` are alternatives: a request is authorized when it carries every
/// scheme of at least one of them. An empty requirement allows anonymous access.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationSecurity {
  pub schemes: Vec<CredentialScheme>,
  pub requirements: Vec<Vec<FieldNameToken>>,
}

impl OperationSecurity {
  /// Whether any accepted API key travels in `location`.
  #[must_use]
  pub fn reads(&self, location: ApiKeyLocation) -> bool {
    self
      .schemes
      .iter()
      .any(|scheme| scheme.api_key_location() == Some(location))
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
