//! The petstore document covers `OpenAPI` 3.1.2.
//!
//! The other `petstore_*` targets prove the generated client and server honor each feature;
//! this one proves the document uses each feature at all. Every case names one `OpenAPI` 3.1.2
//! object, field, or JSON Schema 2020-12 keyword and finds it in `fixtures/petstore.json`, so
//! removing a feature from the document fails here before its behavior goes untested.

use std::sync::LazyLock;

use rstest::rstest;
use serde_json::Value;

static DOCUMENT: LazyLock<Value> =
  LazyLock::new(|| serde_json::from_str(include_str!("../fixtures/petstore.json")).expect("petstore.json is JSON"));

/// Every object in the document, depth first.
fn objects(value: &Value) -> Vec<&serde_json::Map<String, Value>> {
  let mut found = vec![];
  let mut pending = vec![value];
  while let Some(value) = pending.pop() {
    match value {
      Value::Object(object) => {
        found.push(object);
        pending.extend(object.values());
      }
      Value::Array(items) => pending.extend(items),
      _ => {}
    }
  }
  found
}

/// Every operation in `paths`, `webhooks`, and `components.pathItems`, with its method.
fn operations() -> Vec<(&'static str, &'static serde_json::Map<String, Value>)> {
  let path_items = ["/paths", "/webhooks", "/components/pathItems"]
    .into_iter()
    .filter_map(|pointer| DOCUMENT.pointer(pointer).and_then(Value::as_object))
    .flat_map(serde_json::Map::values)
    .filter_map(Value::as_object);
  path_items
    .flat_map(|item| {
      ["get", "put", "post", "delete", "options", "head", "patch", "trace"]
        .into_iter()
        .filter_map(move |method| {
          item
            .get(method)
            .and_then(Value::as_object)
            .map(|operation| (method, operation))
        })
    })
    .collect()
}

/// Every Parameter Object, resolved or inline.
fn parameters() -> Vec<&'static serde_json::Map<String, Value>> {
  objects(&DOCUMENT)
    .into_iter()
    .filter(|object| object.get("in").and_then(Value::as_str).is_some() && object.contains_key("name"))
    .collect()
}

#[test]
fn the_document_is_openapi_3_1_2() {
  assert_eq!(DOCUMENT["openapi"], "3.1.2");
}

#[rstest]
#[case::json_schema_dialect("/jsonSchemaDialect")]
#[case::info_summary("/info/summary")]
#[case::info_terms_of_service("/info/termsOfService")]
#[case::info_contact("/info/contact/email")]
#[case::license_identifier("/info/license/identifier")]
#[case::server_variable_enum("/servers/1/variables/region/enum")]
#[case::server_variable_default("/servers/1/variables/basePath/default")]
#[case::top_level_security("/security/0/ApiKeyAuth")]
#[case::tag_external_docs("/tags/0/externalDocs/url")]
#[case::external_docs("/externalDocs/url")]
#[case::path_level_parameters("/paths/~1pets~1{petId}/parameters/0/$ref")]
#[case::path_item_reference("/paths/~1pets~1{petId}~1vaccinations/$ref")]
#[case::webhook("/webhooks/petAdopted/post")]
#[case::webhook_path_item_reference("/webhooks/inventoryLow/$ref")]
#[case::component_schemas("/components/schemas/Pet")]
#[case::component_responses("/components/responses/NotFound")]
#[case::component_parameters("/components/parameters/PetId")]
#[case::component_examples("/components/examples/NewDog/value")]
#[case::component_request_bodies("/components/requestBodies/NewPet")]
#[case::component_headers("/components/headers/ETag")]
#[case::component_security_schemes("/components/securitySchemes/ApiKeyAuth")]
#[case::component_links("/components/links/GetPetById/operationId")]
#[case::component_callbacks("/components/callbacks/AdoptionStatusChanged")]
#[case::component_path_items("/components/pathItems/PetVaccinations")]
#[case::operation_callback("/paths/~1adoptions/post/callbacks/statusChanged/$ref")]
#[case::response_link("/paths/~1pets/post/responses/201/links/GetCreatedPet/$ref")]
#[case::request_body_reference("/paths/~1pets/post/requestBody/$ref")]
#[case::response_reference("/paths/~1pets/get/responses/default/$ref")]
#[case::header_reference("/paths/~1pets/get/responses/200/headers/x-next/$ref")]
#[case::required_response_header("/paths/~1pets/post/responses/201/headers/Location/required")]
#[case::media_type_examples("/paths/~1pets/get/responses/200/content/application~1json/examples/oneDog/$ref")]
#[case::media_type_example("/paths/~1pets~1{petId}/get/responses/200/content/application~1json/example")]
#[case::alternative_media_types("/paths/~1pets/get/responses/200/content/application~1xml")]
#[case::encoding_content_type(
  "/paths/~1intake~1{formId}/post/requestBody/content/multipart~1form-data/encoding/note/contentType"
)]
#[case::encoding_style("/paths/~1intake~1{formId}/post/requestBody/content/multipart~1form-data/encoding/filter/style")]
#[case::encoding_explode(
  "/paths/~1intake~1{formId}/post/requestBody/content/multipart~1form-data/encoding/colors/explode"
)]
#[case::oauth2_authorization_code("/components/securitySchemes/PetstoreOAuth/flows/authorizationCode/refreshUrl")]
#[case::oauth2_client_credentials("/components/securitySchemes/PetstoreOAuth/flows/clientCredentials/scopes")]
#[case::bearer_format("/components/securitySchemes/BearerAuth/bearerFormat")]
#[case::open_id_connect_url("/components/securitySchemes/OpenIdConnect/openIdConnectUrl")]
#[case::discriminator_mapping("/components/schemas/ContentBlock/discriminator/mapping/text")]
#[case::discriminator_without_mapping("/components/schemas/Health/discriminator/propertyName")]
#[case::ref_with_sibling_default("/components/schemas/ChatPreference/properties/fallback/default")]
#[case::recursive_schema("/components/schemas/Lineage/properties/parents/items/$ref")]
#[case::specification_extension("/components/schemas/Species/x-enum-descriptions")]
fn the_document_declares(#[case] pointer: &str) {
  assert!(DOCUMENT.pointer(pointer).is_some(), "missing {pointer}");
}

#[rstest]
#[case::get("get")]
#[case::put("put")]
#[case::post("post")]
#[case::delete("delete")]
#[case::options("options")]
#[case::head("head")]
#[case::patch("patch")]
#[case::trace("trace")]
fn an_operation_uses_every_http_method(#[case] method: &str) {
  assert!(
    operations().iter().any(|(declared, _)| *declared == method),
    "no {method} operation"
  );
}

#[rstest]
#[case::deprecated("deprecated")]
#[case::callbacks("callbacks")]
#[case::tags("tags")]
#[case::summary("summary")]
#[case::description("description")]
#[case::security("security")]
#[case::request_body("requestBody")]
#[case::parameters("parameters")]
fn an_operation_declares(#[case] field: &str) {
  assert!(
    operations().iter().any(|(_, operation)| operation.contains_key(field)),
    "no operation declares {field}"
  );
}

#[rstest]
#[case::path("path")]
#[case::query("query")]
#[case::header("header")]
#[case::cookie("cookie")]
fn a_parameter_uses_every_location(#[case] location: &str) {
  assert!(
    parameters().iter().any(|parameter| parameter["in"] == location),
    "no {location} parameter"
  );
}

#[rstest]
#[case::form_exploded_by_default(None, None)]
#[case::form_exploded_explicitly(None, Some(true))]
#[case::form_joined(Some("form"), Some(false))]
#[case::space_delimited(Some("spaceDelimited"), Some(false))]
#[case::pipe_delimited(Some("pipeDelimited"), Some(false))]
fn a_query_array_uses_every_style(#[case] style: Option<&str>, #[case] explode: Option<bool>) {
  let found = parameters().iter().any(|parameter| {
    parameter["in"] == "query"
      && parameter.get("schema").and_then(|schema| schema.get("type")) == Some(&Value::from("array"))
      && parameter.get("style").and_then(Value::as_str) == style
      && parameter.get("explode").and_then(Value::as_bool) == explode
  });
  assert!(found, "no query array with style {style:?} and explode {explode:?}");
}

#[rstest]
#[case::required("required")]
#[case::allow_reserved("allowReserved")]
#[case::example("example")]
#[case::description("description")]
#[case::explode("explode")]
#[case::style("style")]
fn a_parameter_declares(#[case] field: &str) {
  assert!(
    parameters().iter().any(|parameter| parameter.contains_key(field)),
    "no parameter declares {field}"
  );
}

#[rstest]
#[case::json("application/json")]
#[case::merge_patch("application/merge-patch+json")]
#[case::problem("application/problem+json")]
#[case::xml("application/xml")]
#[case::form("application/x-www-form-urlencoded")]
#[case::multipart("multipart/form-data")]
#[case::text("text/plain")]
#[case::binary("image/png")]
#[case::server_sent_events("text/event-stream")]
fn a_body_uses_every_media_type_category(#[case] media_type: &str) {
  let found = objects(&DOCUMENT)
    .into_iter()
    .filter_map(|object| object.get("content").and_then(Value::as_object))
    .any(|content| content.contains_key(media_type));
  assert!(found, "no body is {media_type}");
}

#[rstest]
#[case::ok("200")]
#[case::created("201")]
#[case::accepted("202")]
#[case::no_content("204")]
#[case::not_modified("304")]
#[case::bad_request("400")]
#[case::unauthorized("401")]
#[case::not_found("404")]
#[case::conflict("409")]
#[case::unprocessable("422")]
#[case::client_error_range("4XX")]
#[case::default("default")]
fn a_response_uses_every_status_form(#[case] status: &str) {
  assert!(
    operations().iter().any(|(_, operation)| operation
      .get("responses")
      .and_then(|responses| responses.get(status))
      .is_some()),
    "no {status} response"
  );
}

#[rstest]
#[case::api_key_header("apiKey", Some("header"), None)]
#[case::api_key_query("apiKey", Some("query"), None)]
#[case::api_key_cookie("apiKey", Some("cookie"), None)]
#[case::http_bearer("http", None, Some("bearer"))]
#[case::http_basic("http", None, Some("basic"))]
#[case::oauth2("oauth2", None, None)]
#[case::open_id_connect("openIdConnect", None, None)]
#[case::mutual_tls("mutualTLS", None, None)]
fn a_security_scheme_uses_every_type(
  #[case] scheme_type: &str,
  #[case] location: Option<&str>,
  #[case] http_scheme: Option<&str>,
) {
  let schemes = DOCUMENT["components"]["securitySchemes"]
    .as_object()
    .expect("security schemes");
  let found = schemes.values().any(|scheme| {
    scheme["type"] == scheme_type
      && location.is_none_or(|location| scheme["in"] == location)
      && http_scheme.is_none_or(|http_scheme| scheme["scheme"] == http_scheme)
  });
  assert!(found, "no {scheme_type} scheme ({location:?}, {http_scheme:?})");
}

#[rstest]
#[case::cleared(|requirements: &[Value]| requirements.is_empty())]
#[case::anonymous_alternative(|requirements: &[Value]| requirements.iter().any(|requirement| requirement.as_object().is_some_and(serde_json::Map::is_empty)))]
#[case::all_of_two_schemes(|requirements: &[Value]| requirements.iter().any(|requirement| requirement.as_object().is_some_and(|schemes| schemes.len() == 2)))]
#[case::one_of_alternatives(|requirements: &[Value]| requirements.len() >= 2)]
#[case::scopes(|requirements: &[Value]| requirements.iter().any(|requirement| requirement.as_object().is_some_and(|schemes| schemes.values().any(|scopes| scopes.as_array().is_some_and(|scopes| !scopes.is_empty())))))]
fn an_operation_uses_every_security_requirement_form(#[case] form: fn(&[Value]) -> bool) {
  let found = operations().iter().any(|(_, operation)| {
    operation
      .get("security")
      .and_then(Value::as_array)
      .is_some_and(|requirements| form(requirements))
  });
  assert!(found);
}

#[rstest]
#[case::all_of("allOf")]
#[case::any_of("anyOf")]
#[case::one_of("oneOf")]
#[case::not("not")]
#[case::if_then_else("if")]
#[case::then("then")]
#[case::else_branch("else")]
#[case::const_value("const")]
#[case::enum_values("enum")]
#[case::properties("properties")]
#[case::additional_properties("additionalProperties")]
#[case::pattern_properties("patternProperties")]
#[case::property_names("propertyNames")]
#[case::max_properties("maxProperties")]
#[case::dependent_required("dependentRequired")]
#[case::unevaluated_properties("unevaluatedProperties")]
#[case::prefix_items("prefixItems")]
#[case::items("items")]
#[case::contains("contains")]
#[case::min_contains("minContains")]
#[case::min_items("minItems")]
#[case::max_items("maxItems")]
#[case::unique_items("uniqueItems")]
#[case::min_length("minLength")]
#[case::max_length("maxLength")]
#[case::pattern("pattern")]
#[case::minimum("minimum")]
#[case::maximum("maximum")]
#[case::exclusive_minimum("exclusiveMinimum")]
#[case::exclusive_maximum("exclusiveMaximum")]
#[case::multiple_of("multipleOf")]
#[case::required("required")]
#[case::read_only("readOnly")]
#[case::write_only("writeOnly")]
#[case::deprecated("deprecated")]
#[case::default("default")]
#[case::examples("examples")]
#[case::title("title")]
#[case::comment("$comment")]
#[case::content_encoding("contentEncoding")]
#[case::content_media_type("contentMediaType")]
#[case::discriminator("discriminator")]
fn a_schema_uses_every_keyword(#[case] keyword: &str) {
  let schemas = &DOCUMENT["components"]["schemas"];
  let paths = &DOCUMENT["paths"];
  let found = [schemas, paths]
    .into_iter()
    .flat_map(objects)
    .any(|object| object.contains_key(keyword));
  assert!(found, "no schema uses {keyword}");
}

#[rstest]
#[case::int32("int32")]
#[case::int64("int64")]
#[case::float("float")]
#[case::double("double")]
#[case::date("date")]
#[case::date_time("date-time")]
#[case::time("time")]
#[case::uuid("uuid")]
#[case::uri("uri")]
#[case::uri_reference("uri-reference")]
#[case::email("email")]
#[case::ipv4("ipv4")]
#[case::hostname("hostname")]
#[case::byte("byte")]
#[case::binary("binary")]
fn a_schema_uses_every_format(#[case] format: &str) {
  let found = objects(&DOCUMENT)
    .into_iter()
    .any(|object| object.get("format").and_then(Value::as_str) == Some(format));
  assert!(found, "no schema uses format {format}");
}

#[rstest]
#[case::strings(|values: &[Value]| values.iter().all(Value::is_string))]
#[case::integers(|values: &[Value]| values.iter().all(Value::is_i64))]
#[case::numbers(|values: &[Value]| values.iter().all(Value::is_number) && values.iter().any(Value::is_f64))]
#[case::booleans(|values: &[Value]| values.iter().all(Value::is_boolean))]
#[case::mixed(|values: &[Value]| values.iter().any(Value::is_string) && values.iter().any(Value::is_number) && values.iter().any(Value::is_boolean))]
#[case::single_value(|values: &[Value]| values.len() == 1)]
fn a_schema_uses_every_enum_shape(#[case] shape: fn(&[Value]) -> bool) {
  let found = objects(&DOCUMENT)
    .into_iter()
    .filter_map(|object| object.get("enum").and_then(Value::as_array))
    .any(|values| shape(values));
  assert!(found);
}

#[rstest]
#[case::nullable_string(&["string", "null"])]
#[case::nullable_number(&["number", "null"])]
#[case::nullable_integer(&["integer", "null"])]
#[case::several_types(&["integer", "string"])]
fn a_schema_uses_a_type_array(#[case] types: &[&str]) {
  let expected = Value::from(types.to_vec());
  let found = objects(&DOCUMENT)
    .into_iter()
    .any(|object| object.get("type") == Some(&expected));
  assert!(found, "no schema has type {types:?}");
}

#[rstest]
#[case::null_branch_in_any_of("anyOf")]
#[case::null_branch_in_one_of("oneOf")]
fn a_union_has_a_null_branch(#[case] keyword: &str) {
  let null = serde_json::json!({ "type": "null" });
  let found = objects(&DOCUMENT)
    .into_iter()
    .filter_map(|object| object.get(keyword).and_then(Value::as_array))
    .any(|branches| branches.contains(&null));
  assert!(found, "no {keyword} has a null branch");
}
