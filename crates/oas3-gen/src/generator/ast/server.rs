use http::Method;

use super::{
  ContentCategory, Documentation, FieldDef, FieldNameToken, FileHeaderNode, MethodNameToken, MultipartFieldInfo,
  OperationInfo, OperationResponse, OperationSecurity, ParameterLocation, ParsedPath, ResponseEnumDef, StructToken,
  TypeRef,
};
use crate::generator::{
  ast::tokens::TraitToken,
  naming::{identifiers::to_rust_type_name, operations::credentials_field_name},
};

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct HandlerBodyInfo {
  pub body_type: TypeRef,
  pub content_category: ContentCategory,
  #[builder(default)]
  pub optional: bool,
  /// Parts a `multipart/form-data` body's struct fields arrive in; `None` when the body has no struct
  pub multipart_fields: Option<Vec<MultipartFieldInfo>>,
}

/// The credentials struct a handler extracts, the request field it fills, and the
/// security it enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerCredentials {
  pub type_name: StructToken,
  pub field: FieldNameToken,
  pub security: OperationSecurity,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerTraitMethod {
  pub name: MethodNameToken,
  #[builder(default)]
  pub docs: Documentation,
  pub request_type: Option<StructToken>,
  pub response: Option<OperationResponse>,
  pub http_method: Method,
  pub path: ParsedPath,
  pub path_params_type: Option<StructToken>,
  pub query_params_type: Option<StructToken>,
  pub header_params_type: Option<StructToken>,
  pub body_info: Option<HandlerBodyInfo>,
  pub credentials: Option<HandlerCredentials>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerRequestTraitDef {
  pub name: TraitToken,
  #[builder(default)]
  pub docs: Documentation,
  #[builder(default)]
  pub methods: Vec<ServerTraitMethod>,
  pub response_enum: Option<ResponseEnumDef>,
}

impl ServerRequestTraitDef {
  /// Builds the server trait definition from converted operations.
  ///
  /// Creates a trait (named `ApiServer`, or `api_name` when provided) with one
  /// method per operation, including typed path, query, and header parameter
  /// structs. Returns `None` if there are no operations to include.
  pub fn from_operations(
    operations: &[OperationInfo],
    api_name: Option<&str>,
    response_enum: Option<ResponseEnumDef>,
  ) -> Option<Self> {
    if operations.is_empty() {
      return None;
    }

    let methods = operations
      .iter()
      .map(|info| {
        let path_params_type =
          extract_nested_type(&info.parameters, ParameterLocation::Path, info.request_type.as_ref());
        let query_params_type =
          extract_nested_type(&info.parameters, ParameterLocation::Query, info.request_type.as_ref());
        let header_params_type =
          extract_nested_type(&info.parameters, ParameterLocation::Header, info.request_type.as_ref());

        let credentials = info
          .credentials_type
          .clone()
          .zip(info.security.clone())
          .map(|(type_name, security)| {
            let parameters = info.parameters.iter().map(|p| p.name.as_str()).collect::<Vec<_>>();
            HandlerCredentials {
              type_name,
              field: FieldNameToken::new(credentials_field_name(&parameters)),
              security,
            }
          });

        let body_info = info.body.as_ref().and_then(|body| {
          body.body_type.as_ref().map(|body_type| {
            HandlerBodyInfo::builder()
              .body_type(body_type.clone())
              .content_category(body.content_category)
              .optional(body.optional)
              .maybe_multipart_fields(body.multipart_fields.clone())
              .build()
          })
        });

        ServerTraitMethod::builder()
          .name(MethodNameToken::from_raw(&info.stable_id))
          .docs(info.documentation.clone())
          .maybe_request_type(info.request_type.clone())
          .maybe_response(info.response.clone())
          .http_method(info.method.clone())
          .path(info.path.clone())
          .maybe_path_params_type(path_params_type)
          .maybe_query_params_type(query_params_type)
          .maybe_header_params_type(header_params_type)
          .maybe_body_info(body_info)
          .maybe_credentials(credentials)
          .build()
      })
      .collect::<Vec<_>>();

    Some(
      Self::builder()
        .name(TraitToken::new_server(api_name, ""))
        .methods(methods)
        .maybe_response_enum(response_enum)
        .build(),
    )
  }
}

/// Extracts the nested parameter struct type for a specific location.
///
/// Returns the struct name (e.g., `GetUsersRequestPath`) if any parameters
/// exist for the given location, `None` otherwise.
fn extract_nested_type(
  parameters: &[FieldDef],
  location: ParameterLocation,
  request_type: Option<&StructToken>,
) -> Option<StructToken> {
  let has_params = parameters.iter().any(|p| p.parameter_location == Some(location));
  let suffix = location.suffix()?;

  has_params
    .then(|| request_type.map(|req| StructToken::new(format!("{req}{suffix}"))))
    .flatten()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct ServerRootNode {
  pub header: FileHeaderNode,
}

impl TraitToken {
  pub(crate) fn new_server(name: Option<&str>, fallback: &str) -> Self {
    let trait_name = if let Some(name) = name
      && !name.is_empty()
    {
      name.to_string()
    } else if fallback.is_empty() {
      "ApiServer".to_string()
    } else {
      format!("{}Server", to_rust_type_name(fallback))
    };

    Self::from_raw(trait_name)
  }
}
