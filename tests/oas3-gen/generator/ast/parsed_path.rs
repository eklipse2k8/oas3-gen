use oas3_gen::generator::{
  ast::{
    FieldDef, FieldNameToken, ParameterLocation, ParsedPath, TypeRef,
    parsed_path::{PathParseError, PathSegment},
  },
  naming::identifiers::to_rust_field_name,
};
use proptest::{collection::vec, option, prelude::*};
use rstest::rstest;

#[derive(Debug, Clone)]
enum TemplatePart {
  Literal(String),
  Param(String),
}

fn path_param(original: &str, field: &str) -> FieldDef {
  FieldDef::builder()
    .name(FieldNameToken::new(field))
    .rust_type(TypeRef::new("String"))
    .parameter_location(ParameterLocation::Path)
    .original_name(original.to_string())
    .build()
}

fn segment_shape(segment: &PathSegment) -> char {
  match segment {
    PathSegment::Literal(_) => 'L',
    PathSegment::Param(_) => 'P',
    PathSegment::Mixed { .. } => 'M',
  }
}

fn describe(path: &str, params: &[(&str, &str)]) -> Result<String, PathParseError> {
  let fields = params
    .iter()
    .map(|(original, field)| path_param(original, field))
    .collect::<Vec<_>>();
  let parsed = ParsedPath::parse(path, &fields)?;
  let shape = parsed.segments.iter().map(segment_shape).collect::<String>();
  let query = parsed
    .query_string
    .as_ref()
    .map(|query| format!(" ?{query}"))
    .unwrap_or_default();
  Ok(format!("{} [{shape}]{query}", parsed.to_axum_path()))
}

fn template_part() -> impl Strategy<Value = TemplatePart> {
  prop_oneof![
    "[a-z0-9.~-]{1,5}".prop_map(TemplatePart::Literal),
    "[a-z]{2,4}[A-Z][a-z]{1,4}".prop_map(TemplatePart::Param),
  ]
}

fn render_template(segments: &[Vec<TemplatePart>], param: impl Fn(&str) -> String) -> String {
  let rendered = segments
    .iter()
    .map(|parts| {
      parts
        .iter()
        .map(|part| match part {
          TemplatePart::Literal(literal) => literal.clone(),
          TemplatePart::Param(name) => format!("{{{}}}", param(name)),
        })
        .collect::<String>()
    })
    .collect::<Vec<_>>()
    .join("/");
  format!("/{rendered}")
}

#[rstest]
fn paths_parse_into_axum_routes_or_explain_malformed_templates() {
  for (path, params, expected) in [
    ("/", &[][..], Ok("/ []")),
    ("/api/v1/pets", &[], Ok("/api/v1/pets [LLL]")),
    (
      "/users/{user-id}",
      &[("user-id", "user_id")],
      Ok("/users/{user_id} [LP]"),
    ),
    ("/users/{id}/posts/{id}", &[], Ok("/users/{id}/posts/{id} [LPLP]")),
    (
      "/files/v{api-version}.json",
      &[("api-version", "api_version")],
      Ok("/files/v{api_version}.json [LM]"),
    ),
    ("/{major}.{minor}", &[], Ok("/{major}.{minor} [M]")),
    ("/{a}{b}/{c}", &[], Ok("/{a}{b}/{c} [MP]")),
    (
      "/v1/batches/{batch_id}?beta=true",
      &[("batch_id", "batch_id")],
      Ok("/v1/batches/{batch_id} [LLP] ?beta=true"),
    ),
    (
      "/api/endpoint?foo=bar&baz=qux",
      &[],
      Ok("/api/endpoint [LL] ?foo=bar&baz=qux"),
    ),
    (
      "/valid/{unclosed/other",
      &[],
      Err("unclosed '{' at position 0 in segment '{unclosed'"),
    ),
    ("/foo/{}/bar", &[], Err("empty parameter '{}' in segment '{}'")),
    ("/foo}bar", &[], Err("unmatched '}' at position 3 in segment 'foo}bar'")),
    (
      "/{outer{inner}}",
      &[],
      Err("nested '{' at position 6 in segment '{outer{inner}}'"),
    ),
  ] {
    assert_eq!(
      describe(path, params).map_err(|error| error.to_string()),
      expected.map(ToString::to_string).map_err(ToString::to_string),
      "parsing {path}"
    );
  }

  for (path, expected) in [
    (
      "/projects/{projectKey}/repos/{repositorySlug}",
      &["projectKey", "repositorySlug"][..],
    ),
    ("/api/v1/status", &[]),
    ("/foo/{}/bar/{id}", &["id"]),
    ("/foo/{unclosed", &[]),
  ] {
    assert_eq!(
      ParsedPath::extract_template_params(path).collect::<Vec<_>>(),
      expected,
      "template params of {path}"
    );
  }
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(128))]

  #[test]
  fn path_templates_round_trip_with_sanitized_param_names(
    segments in vec(vec(template_part(), 1..4), 0..5),
    query in option::of("[a-z]{1,4}=[a-z0-9]{1,4}"),
  ) {
    let template = render_template(&segments, ToString::to_string);
    let expected_route = render_template(&segments, to_rust_field_name);
    let names = segments
      .iter()
      .flatten()
      .filter_map(|part| match part {
        TemplatePart::Param(name) => Some(name.as_str()),
        TemplatePart::Literal(_) => None,
      })
      .collect::<Vec<_>>();
    let fields = names
      .iter()
      .map(|name| path_param(name, &to_rust_field_name(name)))
      .collect::<Vec<_>>();
    let path = query.as_ref().map_or_else(|| template.clone(), |query| format!("{template}?{query}"));

    prop_assert_eq!(ParsedPath::extract_template_params(&path).collect::<Vec<_>>(), names);
    let parsed = ParsedPath::parse(&path, &fields).map_err(|error| TestCaseError::fail(error.to_string()))?;
    prop_assert_eq!(parsed.to_axum_path(), expected_route);
    prop_assert_eq!(parsed.query_string, query);
  }
}
