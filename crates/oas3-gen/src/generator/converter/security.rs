use std::rc::Rc;

use indexmap::IndexMap;
use itertools::Itertools;
use oas3::spec::{Operation, SecurityRequirement, SecurityScheme};

use super::{ConverterContext, GenerationTarget};
use crate::generator::{
  ast::{
    CredentialKind, CredentialScheme, Documentation, FieldNameToken, OperationInfo, OperationSecurity, RustType,
    StructDef, StructKind, StructToken,
  },
  metrics::GenerationWarning,
  naming::{
    constants::CLIENT_MEMBER_NAMES,
    operations::{credential_field_names, credentials_name},
  },
};

/// A declared security scheme: the credential it carries, or why it carries none.
type DeclaredScheme = Result<CredentialScheme, String>;

const UNDECLARED_SCHEME: &str = "it is not declared in `components.securitySchemes`";

const SUPPORTED_SCHEMES: &str = "only `apiKey` schemes and `http` schemes using `bearer` are generated";

/// Resolves the `apiKey` and bearer security requirements that apply to each
/// operation, and builds the server's credentials structs.
///
/// Only `apiKey` schemes and `http` schemes using `bearer` produce credentials. A
/// requirement that names any other kind of scheme can't be checked by generated
/// code, so it leaves every credential of the operation optional, including the
/// ones it names itself.
#[derive(Debug, Clone)]
pub(crate) struct SecurityConverter {
  context: Rc<ConverterContext>,
  declared: IndexMap<String, DeclaredScheme>,
  global: Vec<SecurityRequirement>,
}

impl SecurityConverter {
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    let spec = context.graph().spec();
    let schemes = spec
      .components
      .iter()
      .flat_map(|components| &components.security_schemes)
      .collect::<Vec<_>>();
    let reserved = match context.config().target {
      GenerationTarget::Client => CLIENT_MEMBER_NAMES,
      GenerationTarget::Server => &[],
    };
    let fields = credential_field_names(schemes.iter().map(|(name, _)| name.as_str()), reserved);

    let declared = schemes
      .into_iter()
      .zip(fields)
      .map(|((name, scheme), field)| {
        let scheme = scheme
          .resolve(spec)
          .map_err(|error| format!("it does not resolve: {error}"))
          .and_then(|scheme| credential_scheme(name, field, scheme));
        (name.clone(), scheme)
      })
      .collect();

    Self {
      context: context.clone(),
      declared,
      global: spec.security.clone(),
    }
  }

  /// Resolves the credentials `operation` accepts, or `None` when it accepts none.
  ///
  /// Operation-level `security` replaces the top-level requirements. Schemes keep
  /// their declaration order and requirements are sorted, so operations that list
  /// the same alternatives in a different order share one credentials struct.
  pub(crate) fn convert(&self, operation: &Operation) -> Option<OperationSecurity> {
    let mut schemes = vec![];
    let mut requirements = vec![];
    let mut unchecked = false;

    for requirement in self.requirements(operation) {
      let resolved = requirement.0.keys().map(|name| self.scheme(name)).collect::<Vec<_>>();
      if resolved.iter().all(Result::is_ok) {
        requirements.push(resolved.iter().flatten().map(|scheme| scheme.field.clone()).collect());
      } else {
        unchecked = true;
      }
      schemes.extend(resolved.into_iter().flatten());
    }

    if unchecked {
      requirements.push(vec![]);
    }
    requirements.sort();
    requirements.dedup();

    let schemes = schemes
      .into_iter()
      .unique_by(|scheme| &scheme.scheme_name)
      .sorted_by_key(|scheme| self.declared.get_index_of(&scheme.scheme_name))
      .cloned()
      .collect::<Vec<_>>();
    (!schemes.is_empty()).then_some(OperationSecurity { schemes, requirements })
  }

  /// Returns the server credentials struct name for `security`, reserving it on first use.
  ///
  /// Operations with the same security share one struct, named after its schemes.
  pub(crate) fn credentials_type(&self, security: &OperationSecurity) -> StructToken {
    let mut cache = self.context.cache_mut();
    if let Some(name) = cache.credentials_type(security) {
      return name.clone();
    }

    let name = cache.make_unique_name(&credentials_name(
      security.schemes.iter().map(|scheme| scheme.scheme_name.as_str()),
    ));
    cache.mark_name_used(name.clone());
    let name = StructToken::new(name);
    cache.register_credentials(security.clone(), name.clone());
    name
  }

  /// Warns once for each security scheme the operations reference but get no
  /// credentials for.
  pub(crate) fn unsupported_warnings<'a>(
    &self,
    operations: impl IntoIterator<Item = &'a Operation>,
  ) -> Vec<GenerationWarning> {
    let mut reasons = IndexMap::<&str, &str>::new();

    for operation in operations {
      for name in self
        .requirements(operation)
        .iter()
        .flat_map(|requirement| requirement.0.keys())
      {
        if let Err(reason) = self.scheme(name) {
          reasons.entry(name).or_insert(reason);
        }
      }
    }

    reasons
      .into_iter()
      .map(|(scheme_name, reason)| GenerationWarning::UnsupportedSecurityScheme {
        scheme_name: scheme_name.to_string(),
        reason: reason.to_string(),
      })
      .collect()
  }

  /// The credential scheme declared as `name`, or why it carries no credentials.
  fn scheme(&self, name: &str) -> Result<&CredentialScheme, &str> {
    match self.declared.get(name) {
      Some(Ok(scheme)) => Ok(scheme),
      Some(Err(reason)) => Err(reason),
      None => Err(UNDECLARED_SCHEME),
    }
  }

  fn requirements<'a>(&'a self, operation: &'a Operation) -> &'a [SecurityRequirement] {
    if operation.security.is_empty() {
      &self.global
    } else {
      &operation.security
    }
  }
}

/// The credentials structs that `operations` use, once each, so an operation that
/// failed to convert leaves no struct behind.
pub(crate) fn credentials_structs(operations: &[OperationInfo]) -> impl Iterator<Item = RustType> {
  operations
    .iter()
    .filter_map(|operation| operation.credentials_type.as_ref().zip(operation.security.as_ref()))
    .unique_by(|(name, _)| *name)
    .map(|(name, security)| {
      RustType::Struct(
        StructDef::builder()
          .name(name.clone())
          .docs(Documentation::from_lines([
            "Credentials an operation accepts, read from the request before the operation runs.",
          ]))
          .fields(security.credential_fields())
          .kind(StructKind::Credentials)
          .build(),
      )
    })
}

fn credential_scheme(name: &str, field: String, scheme: SecurityScheme) -> DeclaredScheme {
  let (kind, description) = match scheme {
    SecurityScheme::ApiKey {
      description,
      name: parameter_name,
      location,
    } => {
      let location = location
        .parse()
        .map_err(|_| format!("its API key location `{location}` is not header, query, or cookie"))?;
      let kind = CredentialKind::ApiKey {
        location,
        parameter_name,
      };
      (kind, description)
    }
    SecurityScheme::Http {
      description, scheme, ..
    } if scheme.eq_ignore_ascii_case("bearer") => (CredentialKind::Bearer, description),
    SecurityScheme::Http { scheme, .. } => {
      return Err(format!(
        "{SUPPORTED_SCHEMES}, and it has type `http` with scheme `{scheme}`"
      ));
    }
    SecurityScheme::OAuth2 { .. } => return Err(format!("{SUPPORTED_SCHEMES}, and it has type `oauth2`")),
    SecurityScheme::OpenIdConnect { .. } => {
      return Err(format!("{SUPPORTED_SCHEMES}, and it has type `openIdConnect`"));
    }
    SecurityScheme::MutualTls { .. } => return Err(format!("{SUPPORTED_SCHEMES}, and it has type `mutualTLS`")),
  };
  Ok(CredentialScheme {
    scheme_name: name.to_string(),
    field: FieldNameToken::from_raw(field),
    kind,
    description,
  })
}
