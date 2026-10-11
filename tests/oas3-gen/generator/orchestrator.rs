use std::{
  collections::{BTreeSet, HashMap},
  sync::LazyLock,
};

use oas3::Spec;
use oas3_gen::{
  generator::{
    ClientModMode, CodegenConfig, GenerationTarget, SchemaScope, ServerModMode, TypesMode,
    ast::OperationKind,
    codegen::{GeneratedFileType, Visibility},
    operation_registry::OperationRegistry,
    orchestrator::{GeneratedFinalOutput, Orchestrator},
  },
  utils::spec::SpecFormat,
};
use proptest::{collection::vec, prelude::*};
use rstest::rstest;
use serde_json::Value;

use crate::{
  assert_contains, assert_not_contains, field_attributes, generated_code, make_orchestrator_with_config, string_set,
};

const SOURCE: &str = "spec.json";
const PETSTORE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/petstore.json"));
const FILTERED_OPERATIONS: [(&str, &str, &str); 4] = [
  ("list_pets", "ListPetsRequest", "PetPage"),
  ("adopt_pet", "AdoptPetRequest", "AdoptionForm"),
  ("place_order", "PlaceOrderRequest", "OrderReceipt"),
  ("check_health", "CheckHealthRequest", "HealthReport"),
];

static PETSTORE_DOCUMENT: LazyLock<Value> =
  LazyLock::new(|| serde_json::from_str(PETSTORE).expect("the petstore fixture is JSON"));

pub(super) fn defined_types(code: &str) -> BTreeSet<&str> {
  code
    .lines()
    .filter_map(|line| {
      let rest = ["pub struct ", "pub enum ", "pub type "]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))?;
      let end = rest
        .find(|character: char| !(character.is_alphanumeric() || character == '_'))
        .unwrap_or(rest.len());
      Some(&rest[..end])
    })
    .collect()
}

#[track_caller]
pub(super) fn struct_fields<'a>(code: &'a str, name: &str) -> Vec<&'a str> {
  let header = format!("pub struct {name} {{");
  let Some(start) = code.find(&header) else {
    panic!("missing struct {name}");
  };
  let body = &code[start + header.len()..];
  let body = &body[..body.find("\n}").unwrap_or(body.len())];
  body
    .lines()
    .map(str::trim)
    .filter(|line| line.starts_with("pub "))
    .collect()
}

#[expect(clippy::needless_pass_by_value, reason = "map_err hands over the error by value")]
fn fail(error: anyhow::Error) -> TestCaseError {
  TestCaseError::fail(format!("{error:#}"))
}

fn parse(spec_json: &str) -> anyhow::Result<Spec> {
  SpecFormat::Json.parse(spec_json.as_bytes())
}

fn defaults() -> CodegenConfig {
  CodegenConfig::builder()
    .schema_scope(SchemaScope::All)
    .enable_builders(true)
    .build()
}

fn customized() -> CodegenConfig {
  CodegenConfig {
    customizations: HashMap::from([
      ("date_time".to_string(), "crate::MyDateTime".to_string()),
      ("date".to_string(), "crate::MyDate".to_string()),
      ("uuid".to_string(), "crate::MyUuid".to_string()),
    ]),
    ..defaults()
  }
}

fn renamed() -> CodegenConfig {
  CodegenConfig {
    api_name: Some("PetStoreApi".to_string()),
    fn_name_overrides: HashMap::from([("listPets".to_string(), "fetchAllPets".to_string())]),
    ..defaults()
  }
}

fn generate(spec: &Spec, config: CodegenConfig) -> anyhow::Result<GeneratedFinalOutput> {
  let target = config.target;
  let orchestrator = make_orchestrator_with_config(spec.clone(), config);
  match target {
    GenerationTarget::Client => orchestrator.generate(&ClientModMode::default(), SOURCE),
    GenerationTarget::Server => orchestrator.generate(&ServerModMode::default(), SOURCE),
  }
}

fn generate_both(spec: &Spec, config: &CodegenConfig) -> anyhow::Result<[GeneratedFinalOutput; 2]> {
  let generate_for = |target| {
    generate(
      spec,
      CodegenConfig {
        target,
        ..config.clone()
      },
    )
  };
  Ok([
    generate_for(GenerationTarget::Client)?,
    generate_for(GenerationTarget::Server)?,
  ])
}

fn render_both(spec_json: &str, config: &CodegenConfig) -> anyhow::Result<String> {
  let [client, server] = generate_both(&parse(spec_json)?, config)?;
  Ok(
    [
      generated_code(&client, &GeneratedFileType::Types),
      generated_code(&client, &GeneratedFileType::Client),
      generated_code(&server, &GeneratedFileType::Types),
      generated_code(&server, &GeneratedFileType::Server),
    ]
    .join("\n"),
  )
}

fn reorder(entries: &mut Value, order: &[usize]) {
  let Value::Object(map) = entries else {
    return;
  };
  let pairs = std::mem::take(map).into_iter().collect::<Vec<_>>();
  *map = order.iter().map(|index| pairs[*index].clone()).collect();
}

fn entry_count(pointer: &str) -> usize {
  PETSTORE_DOCUMENT
    .pointer(pointer)
    .and_then(Value::as_object)
    .map_or(0, serde_json::Map::len)
}

fn shuffled(len: usize) -> impl Strategy<Value = Vec<usize>> {
  Just((0..len).collect::<Vec<_>>()).prop_shuffle()
}

#[rstest]
#[case::api_and_function_names_replace_derived_names(
  spec!("generator/naming_overrides.json"),
  renamed(),
  &[
    "pub struct PetStoreApi {",
    "impl PetStoreApi {",
    "pub trait PetStoreApi: Send + Sync",
    "S: PetStoreApi + Clone + Send + Sync + 'static",
    "pub async fn fetch_all_pets(",
    "pub struct FetchAllPetsRequest {",
  ],
  &["SwaggerPetstore", "ApiServer", "list_pets", "ListPets"]
)]
#[case::credential_names_avoid_schemas_parameters_and_client_members(
  spec!("generator/credential_collisions.json"),
  defaults(),
  &[
    "pub struct ApiKeyAuthCredentials {\n    pub label: Option<String>,",
    "pub credentials: ApiKeyAuthCredentials2,",
    "pub credentials_2: ApiKeyAuthCredentials3,",
    "pub struct ClientAndCookieACredentials {",
    "pub client: Option<secrecy::SecretString>,",
    "pub client_2: Option<http::HeaderValue>,",
    "self.set_security(req_builder, &[Security::Client2]);",
  ],
  &["CookieAAndClientCredentials", "ClientAndCookieACredentials2", "Security::BasicAuth"]
)]
#[case::accept_content_type_and_authorization_header_parameters_are_ignored(
  spec!("generator/ignored_header_parameters.json"),
  defaults(),
  &["pub struct ForwardedRequestHeader {\n    pub if_match: Option<String>,\n}", "pub accept: Option<String>,"],
  &["pub authorization:", "pub content_type:"]
)]
#[case::identical_inline_enums_share_one_type_whatever_their_value_order(
  spec!("generator/enum_dedup.json"),
  defaults(),
  &["pub enum Status {", "pub status: Option<Status>,"],
  &["StructAStatus", "StructBStatus", "StructCStatus"]
)]
#[case::customized_formats_deserialize_through_serde_as(
  spec!("generator/customizations.json"),
  customized(),
  &[
    "#[serde_with::serde_as]",
    "#[serde_as(as = \"crate::MyDateTime\")]\n    pub created_at:",
    "#[serde_as(as = \"Option<crate::MyDateTime>\")]\n    pub updated_at:",
    "#[serde_as(as = \"crate::MyDate\")]\n    pub birth_date:",
    "#[serde_as(as = \"crate::MyUuid\")]\n    pub tag:",
    "#[serde_as(as = \"Vec<crate::MyDateTime>\")]\n    pub toebeans:",
  ],
  &[]
)]
#[case::only_types_nothing_else_uses_become_event_frames(
  spec!("generator/event_frames.json"),
  defaults(),
  &[
    "pub type Frame = oas3_gen_support::SseFrame<Payload, Option<FrameEvent>>;",
    "pub type JsonFrame = oas3_gen_support::SseFrame<Payload>;",
    "\"json\" => frame.json().map(Self::Json),",
    "::frames(req, Some(\"[STOP]\"))",
    "pub struct JsonOnly {",
    "pub struct Shared {",
    "oas3_gen_support::SseFrame<String, Option<SharedEvent>>",
    "pub type Named = oas3_gen_support::SseFrame<String>;",
    "pub type Unnamed = oas3_gen_support::SseFrame<i64>;",
    "\"unnamed\" => frame.json().map(Self::Unnamed),",
    "oas3_gen_support::EventStream<oas3_gen_support::SseFrame<Nested>>",
    "Mapped::Named(member) => Self::try_from(member),",
    "Self::try_from(member).map(|sse| sse.event(\"unnamed\"))",
    "|item| axum::response::sse::Event::default().json_data(item)",
  ],
  &[
    "[HALT]",
    "SseFrame<JsonOnly",
    "FromSseFrame for Nested",
    "pub type NestedA",
    "impl TryFrom<Nested>",
    "Option<&'static str>>;",
  ]
)]
#[case::undeclared_path_template_parameters_are_synthesized(
  spec!("generator/undeclared_path_parameters.json"),
  defaults(),
  &[
    "pub struct GetRepoStatusRequestPath {\n    pub project_key: String,\n    pub repository_slug: String,\n}",
    "pub path: GetRepoStatusRequestPath,",
  ],
  &[]
)]
#[case::unions_route_by_a_tag_only_when_every_branch_requires_a_distinct_const(
  spec!("generator/const_tag_routing.json"),
  defaults(),
  &[
    "match value.get(\"kind\")",
    "#[serde(untagged)]\npub enum Optional {",
    "#[serde(untagged)]\npub enum Repeated {",
  ],
  &["#[serde(untagged)]\npub enum Tagged {"]
)]
#[case::bodies_are_labelled_with_concrete_utf8_media_types(
  spec!("generator/media_labels.json"),
  defaults(),
  &[
    ".json(&request.body)",
    ".header(reqwest::header::CONTENT_TYPE, \"application/octet-stream\")",
    ".header(reqwest::header::CONTENT_TYPE, \"text/plain; charset=utf-8\")",
    "axum::http::HeaderValue::from_static(\"text/plain\")",
  ],
  &["\"*/*\"", "\"image/*\"", "\"text/*\"", "iso-8859-1"]
)]
fn generated_code_pins_spec_edge_semantics(
  #[case] spec_json: &str,
  #[case] config: CodegenConfig,
  #[case] present: &[&str],
  #[case] absent: &[&str],
) -> anyhow::Result<()> {
  let code = render_both(spec_json, &config)?;
  for token in present {
    assert_contains(&code, token, "generated client or server");
  }
  for token in absent {
    assert_not_contains(&code, token, "generated client and server");
  }
  Ok(())
}

#[rstest]
#[case::multiple_of_validates_only_exactly_representable_steps(
  spec!("generator/multiple_of.json"),
  "#[validate(",
  &[
    ("battery", Some("#[validate(custom(function = oas3_gen_support::multiple_of::<5>))]")),
    ("dose", Some("#[validate(custom(function = oas3_gen_support::multiple_of_ratio::<1, 4>))]")),
    ("precision", Some("#[validate(custom(function = oas3_gen_support::multiple_of_ratio::<3, 2>))]")),
    ("halves", None),
    ("tiny", None),
  ]
)]
#[case::required_fields_of_defaulted_structs_read_null_as_their_default(
  spec!("generator/default_on_null.json"),
  "#[serde_as(",
  &[
    ("collar_id", Some("#[serde_as(as = \"serde_with::DefaultOnNull\")]")),
    ("payload", Some("#[serde_as(as = \"serde_with::DefaultOnNull<serde_with::base64::Base64>\")]")),
    ("tags", Some("#[serde_as(as = \"serde_with::DefaultOnNull<serde_with::SetPreventDuplicates<_>>\")]")),
    ("kind", None),
    ("mode", None),
    ("note", None),
    ("additional_properties", None),
    ("count", Some("#[serde_as(as = \"serde_with::DefaultOnNull\")]")),
    ("step", None),
    ("label", None),
  ]
)]
#[case::defaults_render_only_where_the_target_type_can_hold_them(
  spec!("generator/target_type_defaults.json"),
  "#[default(",
  &[
    ("config", Some("#[default(Some(Default::default()))]")),
    ("inline_shape", None),
    ("explicit_shape", None),
    ("level", None),
    ("shape", None),
  ]
)]
fn client_field_attributes_follow_their_schema(
  #[case] spec_json: &str,
  #[case] attribute: &str,
  #[case] expected: &[(&str, Option<&str>)],
) -> anyhow::Result<()> {
  let output = generate(&parse(spec_json)?, defaults())?;
  let types = generated_code(&output, &GeneratedFileType::Types);
  let actual = expected
    .iter()
    .map(|(field, _)| {
      let found = field_attributes(&types, field)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(attribute));
      (*field, found)
    })
    .collect::<Vec<_>>();

  assert_eq!(actual, expected);
  Ok(())
}

#[rstest]
fn unsupported_security_schemes_warn_once_each() -> anyhow::Result<()> {
  let spec = parse(spec!("generator/security_warnings.json"))?;

  for output in generate_both(&spec, &defaults())? {
    let warnings = output
      .stats
      .warnings
      .iter()
      .map(ToString::to_string)
      .collect::<Vec<_>>();
    assert_eq!(
      warnings,
      [
        "Security scheme 'BasicAuth' gets no credentials: only `apiKey` schemes and `http` schemes using `bearer` are generated, and it has type `http` with scheme `basic`",
        "Security scheme 'Missing' gets no credentials: it is not declared in `components.securitySchemes`",
      ]
    );
  }
  Ok(())
}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(4))]

  #[test]
  fn generation_is_repeatable_and_follows_declaration_order(
    schema_order in shuffled(entry_count("/components/schemas")),
    path_order in shuffled(entry_count("/paths")),
  ) {
    let mut document = PETSTORE_DOCUMENT.clone();
    reorder(&mut document["components"]["schemas"], &schema_order);
    reorder(&mut document["paths"], &path_order);
    let spec = parse(&document.to_string()).map_err(fail)?;

    let [client, server] = generate_both(&spec, &defaults()).map_err(fail)?;
    let [client_again, server_again] = generate_both(&spec, &defaults()).map_err(fail)?;
    prop_assert!(client.code == client_again.code, "client output differs between runs");
    prop_assert!(server.code == server_again.code, "server output differs between runs");

    let types = generated_code(&client, &GeneratedFileType::Types);
    let defined = defined_types(&types);
    let declared = document["components"]["schemas"]
      .as_object()
      .into_iter()
      .flat_map(|schemas| schemas.keys().map(String::as_str))
      .filter(|name| defined.contains(name))
      .collect::<Vec<_>>();
    let emitted = types
      .lines()
      .filter_map(|line| defined_types(line).into_iter().next())
      .filter(|name| declared.contains(name))
      .collect::<Vec<_>>();
    prop_assert_eq!(emitted, declared);

    let client_code = generated_code(&client, &GeneratedFileType::Client);
    let methods = client_code
      .lines()
      .filter_map(|line| line.trim().strip_prefix("pub async fn "))
      .filter_map(|rest| rest.split('(').next())
      .collect::<Vec<_>>();
    let registry = OperationRegistry::new(&spec);
    let registered = registry
      .operations()
      .filter(|entry| entry.kind == OperationKind::Http)
      .map(|entry| entry.stable_id.as_str())
      .filter(|id| methods.contains(id))
      .collect::<Vec<_>>();
    prop_assert_eq!(methods, registered);
  }

}

proptest! {
  #![proptest_config(ProptestConfig::with_cases(48))]

  #[test]
  fn operation_filters_generate_the_selected_operations_and_the_schemas_they_reach(
    chosen in vec(any::<bool>(), FILTERED_OPERATIONS.len()),
    exclude in any::<bool>(),
    all_schemas in any::<bool>(),
  ) {
    let named = FILTERED_OPERATIONS
      .iter()
      .zip(&chosen)
      .filter(|(_, chosen)| **chosen)
      .map(|((id, ..), _)| *id)
      .collect::<Vec<_>>();
    let ids = string_set(&named);
    let (only, excluded) = if exclude { (None, Some(&ids)) } else { (Some(&ids), None) };
    let config = CodegenConfig {
      schema_scope: if all_schemas { SchemaScope::All } else { SchemaScope::ReferencedOnly },
      ..CodegenConfig::default()
    };
    let spec = parse(spec!("generator/operation_filtering.json")).map_err(fail)?;
    let output = Orchestrator::new(spec, Visibility::default(), config, only, excluded)
      .generate(&TypesMode, SOURCE)
      .map_err(fail)?;
    let types = generated_code(&output, &GeneratedFileType::Types);
    let defined = defined_types(&types);

    let selected = chosen.iter().map(|chosen| *chosen != exclude).collect::<Vec<_>>();
    let selected_count = selected.iter().filter(|selected| **selected).count();
    prop_assert_eq!(output.stats.operations_converted, selected_count);
    for ((_, request, schema), selected) in FILTERED_OPERATIONS.iter().zip(&selected) {
      prop_assert_eq!(defined.contains(request), *selected, "request type {}", request);
      prop_assert_eq!(defined.contains(schema), *selected || all_schemas, "schema {}", schema);
    }
    prop_assert_eq!(defined.contains("AuditLog"), all_schemas);
    let orphaned = if all_schemas { 0 } else { FILTERED_OPERATIONS.len() + 1 - selected_count };
    prop_assert_eq!(output.stats.orphaned_schemas_count, orphaned);
  }
}
