use oas3::spec::{Parameter, ParameterStyle};

/// Determine if a query parameter should use exploded form.
///
/// Returns `true` if the parameter explicitly sets `explode: true`, or if
/// `explode` is unset and the style is either unspecified or `form` (the default).
pub(crate) fn query_param_explode(param: &Parameter) -> bool {
  param
    .explode
    .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)))
}
