//! Type-level contracts of the generated petstore types, without HTTP.
//!
//! The generated client derives `Deserialize` for what it receives and `Serialize`
//! for what it sends; the generated server is the mirror image, and types used in
//! both directions derive both. These tests encode and decode spec-shaped JSON with
//! both crates to prove that discriminated and untagged unions route correctly, enums
//! keep their wire names and JSON types, spec defaults fill absent fields, the client
//! reads `null` in a required field of a defaulted struct as the type's default, string
//! formats parse, `additionalProperties` is honored, and whatever the server writes
//! the client reads back unchanged.
//!
//! Constants are not validated on decode. Both crates mark discriminator tags
//! `skip_deserializing` and fill them from the struct's default; the client does the
//! same for every other constant (`ErrorResponse.type`, `ChatPreference.object`,
//! `UsageCounterA.type`). A discriminated union routes on the tag before decoding the
//! variant, so only the union polices it: a struct decoded directly ignores whatever
//! constant the wire carries and re-emits its own when serialized. The server reads
//! non-discriminator constants as sent, so an implementation can reject a misstated
//! one.

use std::fmt::Debug;

use chrono::{DateTime, NaiveDate, NaiveTime, TimeDelta, TimeZone as _, Utc};
use indexmap::{IndexMap, IndexSet};
use petstore_client as client;
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::select};
use rstest::rstest;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use uuid::Uuid;

const PET_ID: Uuid = Uuid::from_u128(0x8f3c_2a4e_1b2c_4d5e_8f90_1234_5678_9abc);

fn decode<T: DeserializeOwned>(wire: &Value) -> T {
  T::deserialize(wire).unwrap_or_else(|error| panic!("{wire} does not decode: {error}"))
}

fn encode<T: Serialize>(value: &T) -> Value {
  serde_json::to_value(value).expect("the value serializes to JSON")
}

/// The members of a JSON object literal, in order.
fn object(value: Value) -> IndexMap<String, Value> {
  let Value::Object(members) = value else {
    panic!("{value} is not a JSON object");
  };
  members.into_iter().collect()
}

/// 2026-10-09T18:30:00Z, built without parsing.
fn evening() -> DateTime<Utc> {
  Utc
    .with_ymd_and_hms(2026, 10, 9, 18, 30, 0)
    .single()
    .expect("a valid instant")
}

fn day(year: i32, month: u32, day: u32) -> NaiveDate {
  NaiveDate::from_ymd_opt(year, month, day).expect("a valid date")
}

fn client_text(text: &str) -> client::TextBlock {
  client::TextBlock {
    text: text.to_string(),
    ..Default::default()
  }
}

#[rstest]
#[case::text_block(json!({ "type": "text", "text": "Hello" }), client::ContentBlock::text("Hello".to_string()))]
#[case::image_block(
  json!({ "type": "image", "source": { "type": "url", "url": "https://cdn.example/a.png" } }),
  client::ContentBlock::image(Box::new(client::ImageSource::url("https://cdn.example/a.png".to_string())))
)]
#[case::code_block(
  json!({ "type": "code", "code": "ls", "language": "sh" }),
  client::ContentBlock::Code(client::CodeBlock { code: "ls".to_string(), language: Some("sh".to_string()), ..Default::default() })
)]
#[case::tool_use_block(
  json!({ "type": "tool_use", "id": "toolu_1", "name": "find_vet", "input": { "city": "Portland" } }),
  client::ContentBlock::ToolUse(client::ToolUseBlock {
    id: "toolu_1".to_string(),
    name: "find_vet".to_string(),
    input: object(json!({ "city": "Portland" })),
    ..Default::default()
  })
)]
#[case::tool_result_block(
  json!({ "type": "tool_result", "tool_use_id": "toolu_1", "content": "done", "is_error": false }),
  client::ContentBlock::ToolResult(client::ToolResultBlock {
    tool_use_id: "toolu_1".to_string(),
    content: client::ToolResultContent::String("done".to_string()),
    is_error: Some(false),
    ..Default::default()
  })
)]
#[case::tool_result_content_image(
  json!({ "type": "image", "source": { "type": "url", "url": "https://cdn.example/b.png" } }),
  client::ToolResultContentBlock::image(Box::new(client::ImageSource::url("https://cdn.example/b.png".to_string())))
)]
#[case::base64_image_source(
  json!({ "type": "base64", "media_type": "image/gif", "data": "R0lG" }),
  client::ImageSource::Base64(client::Base64ImageSource {
    media_type: client::MediaType::ImageGif,
    data: b"GIF".to_vec(),
    ..Default::default()
  })
)]
#[case::url_image_source(
  json!({ "type": "url", "url": "https://cdn.example/c.png" }),
  client::ImageSource::url("https://cdn.example/c.png".to_string())
)]
#[case::citation_annotation(
  json!({ "type": "citation", "start": 0, "end": 4, "build": "b-1" }),
  client::Annotation::Citation(client::CitationAnnotation { start: 0, end: 4, build: "b-1".to_string(), ..Default::default() })
)]
#[case::link_annotation(
  json!({ "type": "link", "start": 5, "end": 9, "url": "https://vet.example" }),
  client::Annotation::Link(client::LinkAnnotation { start: 5, end: 9, url: "https://vet.example".to_string(), ..Default::default() })
)]
#[case::message_start_event(
  json!({ "type": "message_start", "message": { "id": "msg_1", "role": "assistant", "content": [], "model": "chat-1.1" } }),
  client::AssistantEvent::message_start(client::Message {
    id: "msg_1".to_string(),
    role: client::Role::Assistant,
    content: vec![],
    model: "chat-1.1".to_string(),
    stop_reason: None,
    usage: None,
  })
)]
#[case::content_block_start_event(
  json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "code", "code": "ls" } }),
  client::AssistantEvent::ContentBlockStart(client::ContentBlockStartEvent {
    index: 1,
    content_block: Box::new(client::ContentBlock::code("ls".to_string())),
    ..Default::default()
  })
)]
#[case::content_block_delta_event(
  json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "Hi" } }),
  client::AssistantEvent::ContentBlockDelta(client::ContentBlockDeltaEvent {
    index: 1,
    delta: Box::new(client::Delta::text("Hi".to_string())),
    ..Default::default()
  })
)]
#[case::content_block_stop_event(json!({ "type": "content_block_stop", "index": 2 }), client::AssistantEvent::content_block_stop(2))]
#[case::message_stop_event(json!({ "type": "message_stop" }), client::AssistantEvent::message_stop())]
#[case::ping_event(json!({ "type": "ping" }), client::AssistantEvent::ping())]
#[case::text_delta(json!({ "type": "text_delta", "text": "Hi" }), client::Delta::text("Hi".to_string()))]
#[case::input_json_delta(
  json!({ "type": "input_json_delta", "partial_json": "{\"a\":" }),
  client::Delta::input_json("{\"a\":".to_string())
)]
#[case::allergies_health(
  json!({ "type": "allergies", "allergens": ["pollen"] }),
  client::Health::allergies(Some(vec!["pollen".to_string()]))
)]
#[case::diet_health(json!({ "type": "diet", "food": "kibble" }), client::Health::diet(Some("kibble".to_string())))]
fn discriminated_unions_route_by_tag<T: DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[rstest]
#[case::content_block(serde_json::from_value::<client::ContentBlock>, "video")]
#[case::content_block_server(serde_json::from_value::<server::ContentBlock>, "video")]
#[case::tool_result_content_block(serde_json::from_value::<client::ToolResultContentBlock>, "code")]
#[case::image_source(serde_json::from_value::<client::ImageSource>, "file")]
#[case::annotation(serde_json::from_value::<client::Annotation>, "footnote")]
#[case::assistant_event(serde_json::from_value::<client::AssistantEvent>, "message_delta")]
#[case::delta(serde_json::from_value::<client::Delta>, "thinking_delta")]
#[case::health(serde_json::from_value::<client::Health>, "Allergies")]
fn unknown_tags_are_rejected_with_an_error_naming_the_tag<T: Debug>(
  #[case] decode: fn(Value) -> serde_json::Result<T>,
  #[case] tag: &str,
) {
  let error = decode(json!({ "type": tag })).expect_err("an unknown tag is rejected");
  assert!(error.to_string().contains(&format!("'{tag}'")), "{error}");
}

#[rstest]
#[case::content_block(serde_json::from_value::<client::ContentBlock>)]
#[case::image_source(serde_json::from_value::<client::ImageSource>)]
#[case::annotation(serde_json::from_value::<client::Annotation>)]
#[case::assistant_event(serde_json::from_value::<client::AssistantEvent>)]
#[case::delta(serde_json::from_value::<client::Delta>)]
#[case::health(serde_json::from_value::<client::Health>)]
fn untagged_payloads_are_rejected_by_discriminated_unions<T: Debug>(
  #[case] decode: fn(Value) -> serde_json::Result<T>,
) {
  let error = decode(json!({ "text": "no tag" })).expect_err("a payload without a tag is rejected");
  assert!(error.to_string().contains("missing field `type`"), "{error}");
}

#[rstest]
#[case::text_block(client_text("Hi"), json!({ "type": "text", "text": "Hi" }))]
#[case::content_block(client::ContentBlock::code("ls".to_string()), json!({ "type": "code", "code": "ls" }))]
#[case::url_source(
  client::ImageSource::url("https://cdn.example/a.png".to_string()),
  json!({ "type": "url", "url": "https://cdn.example/a.png" })
)]
#[case::link_annotation(
  client::Annotation::Link(client::LinkAnnotation { start: 1, end: 2, url: "https://vet.example".to_string(), ..Default::default() }),
  json!({ "type": "link", "start": 1, "end": 2, "url": "https://vet.example" })
)]
#[case::counter_b(client::UsageCounterUsageCounterKind::b(2), json!({ "output_tokens": 2, "type": "counter_b" }))]
#[case::recipe_list(client::RecipeList::default(), json!({ "type": "object" }))]
#[case::chat_preference(
  client::ChatPreference { model: client::ChatModelId::Chat10, ..Default::default() },
  json!({ "model": "chat-1.0", "object": "chat.preference", "fallback": "chat-1.1", "tier": "flex" })
)]
#[case::ping_event(server::AssistantEvent::ping(), json!({ "type": "ping" }))]
#[case::content_block_stop_event(server::AssistantEvent::content_block_stop(3), json!({ "type": "content_block_stop", "index": 3 }))]
#[case::input_json_delta(
  server::Delta::input_json("{".to_string()),
  json!({ "type": "input_json_delta", "partial_json": "{" })
)]
#[case::diet_health(server::Health::diet(Some("kibble".to_string())), json!({ "type": "diet", "food": "kibble" }))]
#[case::error_response(
  server::ErrorResponse {
    error: server::ErrorDetails { r#type: server::ErrorType::RateLimitError, message: "slow down".to_string() },
    ..Default::default()
  },
  json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "slow down" } })
)]
fn constants_are_emitted_on_serialization<T: Serialize>(#[case] value: T, #[case] wire: Value) {
  assert_eq!(encode(&value), wire);
}

#[rstest]
#[case::text_block(json!({ "type": "image", "text": "Hi" }), client_text("Hi"))]
#[case::text_block_on_the_server(
  json!({ "type": "image", "text": "Hi" }),
  server::TextBlock { text: "Hi".to_string(), ..Default::default() }
)]
#[case::code_block(
  json!({ "type": "tool_use", "code": "ls" }),
  client::CodeBlock { code: "ls".to_string(), ..Default::default() }
)]
#[case::url_source(
  json!({ "type": "base64", "url": "https://cdn.example/a.png" }),
  client::UrlImageSource { url: "https://cdn.example/a.png".to_string(), ..Default::default() }
)]
#[case::citation(
  json!({ "type": "link", "start": 0, "end": 1, "build": "b-1" }),
  client::CitationAnnotation { start: 0, end: 1, build: "b-1".to_string(), ..Default::default() }
)]
#[case::ping_event(json!({ "type": "pong" }), client::PingEvent::default())]
#[case::error_response(
  json!({ "type": "failure", "error": { "type": "api_error", "message": "boom" } }),
  client::ErrorResponse {
    r#type: "error".to_string(),
    error: client::ErrorDetails { r#type: client::ErrorType::ApiError, message: "boom".to_string() },
  }
)]
#[case::chat_preference_object(
  json!({ "model": "chat-1.0", "object": "chat.thread" }),
  client::ChatPreference {
    model: client::ChatModelId::Chat10,
    object: Some("chat.preference".to_string()),
    fallback: Some(client::ChatModel::Chat11),
    tier: Some(client::ChatPreferenceTier::Other2("flex".to_string())),
  }
)]
fn direct_struct_decodes_replace_a_mismatched_constant_with_their_own<T: DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[rstest]
#[case::tool_result_empty_array(json!([]), client::ToolResultContent::Array(vec![]))]
#[case::tool_result_blocks(
  json!([{ "type": "text", "text": "a" }]),
  client::ToolResultContent::Array(vec![client::ToolResultContentBlock::text("a".to_string())])
)]
#[case::tool_result_string(json!("done"), client::ToolResultContent::String("done".to_string()))]
#[case::metadata_object(json!({ "pet": "Biscuit" }), server::MessageMetadata::Object(object(json!({ "pet": "Biscuit" }))))]
#[case::metadata_empty_object(json!({}), server::MessageMetadata::Object(IndexMap::new()))]
#[case::metadata_string(json!("triage"), server::MessageMetadata::String("triage".to_string()))]
#[case::labels_object(json!({ "a": "b" }), client::IntakeFormLabels::Object(IndexMap::from([("a".to_string(), "b".to_string())])))]
#[case::labels_string(json!("x"), client::IntakeFormLabels::String("x".to_string()))]
#[case::chat_model_choice_listed(json!("chat-1.1"), client::ChatModelChoice::chat11())]
#[case::chat_model_choice_unlisted(json!("chat-1.2"), client::ChatModelChoice::String("chat-1.2".to_string()))]
#[case::chat_model_id_listed(json!("chat-1.0"), client::ChatModelId::Chat10)]
#[case::chat_model_id_unlisted(json!("my-model"), client::ChatModelId::Other("my-model".to_string()))]
#[case::tier_listed_other(json!("other"), client::ChatPreferenceTier::Other)]
#[case::tier_unlisted(json!("flex"), client::ChatPreferenceTier::Other2("flex".to_string()))]
#[case::usage_choice_constant(json!("auto"), client::UsageChoice::Auto)]
#[case::usage_choice_object(
  json!({ "input_tokens": 1, "output_tokens": 2 }),
  client::UsageChoice::Usage(client::Usage { input_tokens: 1, output_tokens: 2 })
)]
#[case::usage_choice_string(json!("manual"), client::UsageChoice::String("manual".to_string()))]
#[case::number_or_string_number(json!(0.25), client::NullableStringOrNumber::Number(0.25))]
#[case::number_or_string_integer(json!(3), client::NullableStringOrNumber::Number(3.0))]
#[case::number_or_string_numeric_string(json!("0.25"), client::NullableStringOrNumber::String("0.25".to_string()))]
#[case::number_or_string_null(json!(null), None::<client::NullableStringOrNumber>)]
#[case::model_union_first_enum(
  json!("model-1.1"),
  client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model11)
)]
#[case::model_union_second_enum(
  json!("Legacy 0.1-plus"),
  client::ModelFilterModel::Enum2(client::ModelFilterModelEnum2::Legacy01Plus)
)]
#[case::image_size_object(
  json!({ "width": 1024, "height": 768 }),
  server::ContentRequestImageSize::ImageSize(server::ImageSize { width: Some(1024), height: Some(768) })
)]
#[case::image_size_preset(json!("landscape_16_9"), server::ContentRequestImageSize::landscape169())]
#[case::single_text_block(json!({ "type": "text", "text": "a" }), client::ArrayOrSingle::text_block("a".to_string()))]
#[case::text_block_array(json!([{ "type": "text", "text": "a" }]), client::ArrayOrSingle::Array(vec![client_text("a")]))]
#[case::empty_text_block_array(json!([]), client::ArrayOrSingle::Array(vec![]))]
#[case::nested_citation(
  json!({ "type": "citation", "start": 0, "end": 3, "build": "b-7" }),
  client::HypotheticalVersion::Citation(client::CitationAnnotation { start: 0, end: 3, build: "b-7".to_string(), ..Default::default() })
)]
#[case::nested_link(
  json!({ "type": "link", "start": 0, "end": 3, "url": "https://vet.example" }),
  client::HypotheticalVersion::Link(client::LinkAnnotation { start: 0, end: 3, url: "https://vet.example".to_string(), ..Default::default() })
)]
#[case::counter_a(json!({ "type": "counter_a", "output_tokens": 3 }), client::UsageCounterUsageCounterKind::a(3))]
#[case::counter_b(json!({ "type": "counter_b", "output_tokens": 3 }), client::UsageCounterUsageCounterKind::b(3))]
#[case::counter_b_server(json!({ "type": "counter_b", "output_tokens": 3 }), server::UsageCounterUsageCounterKind::b(3))]
fn untagged_unions_pick_the_variant_the_wire_value_belongs_to<T: DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[rstest]
#[case::snake_case(client::Species::SmallMammal, r#""small_mammal""#)]
#[case::slash(client::MediaType::ImageWebp, r#""image/webp""#)]
#[case::space_and_dots(client::ModelFilterModelEnum2::Legacy01Mini, r#""Legacy 0.1-mini""#)]
#[case::dashes_and_dots(client::ChatModel::Chat10, r#""chat-1.0""#)]
#[case::stop_reason(client::StopReason::StopSequence, r#""stop_sequence""#)]
#[case::response_kind(client::ResponseKind::Text, r#""text""#)]
#[case::listed_tier(client::ChatPreferenceTier::Other, r#""other""#)]
#[case::unlisted_tier(client::ChatPreferenceTier::Other2("flex".to_string()), r#""flex""#)]
#[case::unlisted_model(client::ChatModelId::Other("chat-9".to_string()), r#""chat-9""#)]
#[case::integer(client::SampleRate::Value44100, "44100")]
#[case::integer_without_type(client::Priority::Value3, "3")]
#[case::integer_on_the_server(server::EnergyLevel::Value2, "2")]
#[case::float(client::PlaybackRate::Value1_5, "1.5")]
#[case::whole_float(client::PlaybackRate::Value2, "2.0")]
#[case::boolean_true(client::Toggle::True, "true")]
#[case::boolean_false(client::Toggle::False, "false")]
#[case::mixed_string(client::ResponseQuality::Auto, r#""auto""#)]
#[case::mixed_integer(client::ResponseQuality::Value1, "1")]
#[case::mixed_float(client::ResponseQuality::Value0_5, "0.5")]
#[case::mixed_boolean(client::ResponseQuality::True, "true")]
fn enums_keep_their_wire_names_and_json_types<T: Serialize + DeserializeOwned + PartialEq + Debug>(
  #[case] value: T,
  #[case] wire: &str,
) {
  assert_eq!(serde_json::to_string(&value).expect("serializes"), wire);
  assert_eq!(serde_json::from_str::<T>(wire).expect("deserializes"), value);
}

#[rstest]
#[case::image_size_preset(client::ImageSizePreset::Auto2k, r#""auto_2K""#, server::ImageSizePreset::Auto2k)]
#[case::image_size_portrait(
  client::ImageSizePreset::Portrait169,
  r#""portrait_16_9""#,
  server::ImageSizePreset::Portrait169
)]
#[case::collar_mode(
  client::CollarTelemetryMode::Tracking,
  r#""tracking""#,
  server::CollarTelemetryMode::Tracking
)]
#[case::error_type(
  server::ErrorType::OverloadedError,
  r#""overloaded_error""#,
  client::ErrorType::OverloadedError
)]
#[case::feed_event_type(
  server::AdoptionFeedEventType::Deleted,
  r#""deleted""#,
  client::AdoptionFeedEventType::Deleted
)]
#[case::vitals_activity(
  server::VitalsReadingActivity::Sleeping,
  r#""sleeping""#,
  client::VitalsReadingActivity::Sleeping
)]
fn one_way_enums_are_written_by_one_side_and_read_by_the_other<
  W: Serialize,
  R: DeserializeOwned + PartialEq + Debug,
>(
  #[case] written: W,
  #[case] wire: &str,
  #[case] read: R,
) {
  assert_eq!(serde_json::to_string(&written).expect("serializes"), wire);
  assert_eq!(serde_json::from_str::<R>(wire).expect("deserializes"), read);
}

#[rstest]
#[case::sample_rate(client::SampleRate::Value24000)]
#[case::sample_rate_server(server::SampleRate::Value24000)]
#[case::playback_rate(client::PlaybackRate::Value1)]
#[case::playback_rate_server(server::PlaybackRate::Value1)]
#[case::stop_reason_without_default_takes_the_first_value(client::StopReason::EndTurn)]
#[case::response_kind(client::ResponseKind::Json)]
#[case::usage_choice(client::UsageChoice::Auto)]
#[case::collar_mode(client::CollarTelemetryMode::Standard)]
#[case::tier(client::ChatPreferenceTier::Other2("flex".to_string()))]
#[case::image_size(client::ContentRequestImageSize::Preset(client::ImageSizePreset::Auto2k))]
#[case::model_selection(client::ModelSelection {
  model: Some(client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model10)),
})]
fn defaults_match_the_spec<T: Default + PartialEq + Debug>(#[case] expected: T) {
  assert_eq!(T::default(), expected);
}

#[rstest]
#[case::integer_enum_from_string(serde_json::from_value::<client::SampleRate>, json!("24000"), "invalid type: string")]
#[case::integer_enum_unlisted(serde_json::from_value::<client::SampleRate>, json!(22050), "unknown variant 22050")]
#[case::untyped_integer_enum_from_string(serde_json::from_value::<client::Priority>, json!("1"), "invalid type: string")]
#[case::untyped_integer_enum_unlisted(serde_json::from_value::<client::Priority>, json!(4), "unknown variant 4")]
#[case::server_integer_enum_from_string(serde_json::from_value::<server::EnergyLevel>, json!("2"), "invalid type: string")]
#[case::float_enum_unlisted(serde_json::from_value::<client::PlaybackRate>, json!(0.75), "unknown variant 0.75")]
#[case::float_enum_unlisted_integer(serde_json::from_value::<client::PlaybackRate>, json!(3), "unknown variant 3")]
#[case::float_enum_from_string(serde_json::from_value::<client::PlaybackRate>, json!("1.5"), "invalid type: string")]
#[case::boolean_enum_from_string(serde_json::from_value::<client::Toggle>, json!("true"), "invalid type: string")]
#[case::mixed_enum_unlisted_number(serde_json::from_value::<client::ResponseQuality>, json!(2), "unknown variant 2")]
#[case::mixed_enum_unlisted_boolean(serde_json::from_value::<client::ResponseQuality>, json!(false), "unknown variant false")]
#[case::mixed_enum_wrong_case(serde_json::from_value::<client::ResponseQuality>, json!("AUTO"), "unknown variant")]
#[case::string_enum_wrong_case(serde_json::from_value::<client::Species>, json!("Dog"), "unknown variant `Dog`")]
#[case::string_enum_unlisted_media_type(
  serde_json::from_value::<client::MediaType>,
  json!("image/svg+xml"),
  "unknown variant `image/svg+xml`"
)]
#[case::exhaustive_chat_model(serde_json::from_value::<client::ChatModel>, json!("chat-2.0"), "unknown variant `chat-2.0`")]
fn enums_reject_values_the_spec_does_not_list<T: Debug>(
  #[case] decode: fn(Value) -> serde_json::Result<T>,
  #[case] wire: Value,
  #[case] message: &str,
) {
  let error = decode(wire.clone()).expect_err("the value is rejected");
  assert!(error.to_string().contains(message), "{wire}: {error}");
}

#[rstest]
#[case::order_on_the_client(
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc" }),
  client::Order { id: None, pet_id: PET_ID, quantity: Some(1), ship_date: None, status: None, complete: Some(false) }
)]
#[case::order_on_the_server(
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "status": "approved" }),
  server::Order {
    id: None,
    pet_id: PET_ID,
    quantity: Some(1),
    ship_date: None,
    status: Some(server::OrderStatus::Approved),
    complete: Some(false),
  }
)]
#[case::problem_on_the_client(
  json!({ "title": "Not Found" }),
  client::Problem {
    r#type: Some("about:blank".to_string()),
    title: Some("Not Found".to_string()),
    status: None,
    detail: None,
    instance: None,
    additional_properties: IndexMap::new(),
  }
)]
#[case::problem_on_the_server(
  json!({ "status": 404 }),
  server::Problem {
    r#type: Some("about:blank".to_string()),
    title: None,
    status: Some(404),
    detail: None,
    instance: None,
    additional_properties: IndexMap::new(),
  }
)]
#[case::image_size(json!({}), server::ImageSize { width: Some(512), height: Some(512) })]
#[case::image_size_without_height(json!({ "width": 1024 }), server::ImageSize { width: Some(1024), height: Some(512) })]
#[case::collar_telemetry(
  json!({ "collarId": "COL-0A1B2C3D", "recordedAt": "2026-10-09T18:30:00Z", "location": [45.52, -122.68], "samples": [72] }),
  server::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: evening(),
    location: vec![45.52, -122.68],
    samples: vec![72],
    mode: Some(server::CollarTelemetryMode::Standard),
    ..Default::default()
  }
)]
#[case::chat_preference(
  json!({ "model": "chat-1.0" }),
  client::ChatPreference {
    model: client::ChatModelId::Chat10,
    object: Some("chat.preference".to_string()),
    fallback: Some(client::ChatModel::Chat11),
    tier: Some(client::ChatPreferenceTier::Other2("flex".to_string())),
  }
)]
#[case::content_request_image_size(
  json!({ "blocks": [] }),
  server::ContentRequest {
    blocks: vec![],
    metadata: None,
    image_size: Some(server::ContentRequestImageSize::Preset(server::ImageSizePreset::Auto2k)),
  }
)]
#[case::model_selection(
  json!({}),
  client::ModelSelection { model: Some(client::ModelFilterModel::Enum(client::ModelFilterModelEnum::Model10)) }
)]
#[case::assistant_settings(
  json!({}),
  client::AssistantSettings {
    response_kind: Some(client::ResponseKind::Json),
    usage: Some(client::UsageChoice::Auto),
    sample_rate: Some(client::SampleRate::Value24000),
    playback_rate: Some(client::PlaybackRate::Value1),
    ..Default::default()
  }
)]
fn absent_fields_take_the_spec_default<T: DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[rstest]
#[case::order(
  json!({ "petId": null }),
  client::Order { id: None, pet_id: Uuid::nil(), quantity: Some(1), ship_date: None, status: None, complete: Some(false) }
)]
#[case::base64_image_source(
  json!({ "type": "base64", "media_type": null, "data": null }),
  client::Base64ImageSource::default()
)]
fn null_required_fields_take_the_type_default_on_the_client<T: DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[test]
fn null_required_fields_are_rejected_on_the_server() {
  let wire = json!({ "petId": null });
  assert!(
    serde_json::from_value::<server::Order>(wire.clone()).is_err(),
    "{wire} decoded"
  );
}

#[test]
fn recursive_unions_default_to_a_variant_that_does_not_recurse() {
  assert_eq!(client::Node::default(), client::Node::Leaf, "client Node default");
  assert_eq!(server::Node::default(), server::Node::Leaf, "server Node default");

  let wire = json!({ "node": "leaf", "kind": "w" });
  assert_eq!(
    encode(&decode::<client::Wrapper>(&wire)),
    wire,
    "client Wrapper round trip"
  );
  assert_eq!(
    encode(&decode::<server::Wrapper>(&wire)),
    wire,
    "server Wrapper round trip"
  );
}

#[rstest]
#[case::uuid_and_double(
  json!({ "id": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "name": "Rex", "species": "dog", "weightKg": 12.345 }),
  client::Pet { id: PET_ID, name: "Rex".to_string(), species: client::Species::Dog, weight_kg: Some(12.345), ..Default::default() }
)]
#[case::uppercase_uuid(
  json!({ "petId": "8F3C2A4E-1B2C-4D5E-8F90-123456789ABC", "name": "Rex" }),
  client::Lineage { pet_id: PET_ID, name: "Rex".to_string(), species: None, parents: None }
)]
#[case::date(
  json!({ "vaccine": "rabies", "date_administered": "2026-06-09", "nextDue": "2027-06-09" }),
  client::VaccineRecord {
    vaccine: "rabies".to_string(),
    date_administered: day(2026, 6, 9),
    veterinarian: None,
    next_due: Some(day(2027, 6, 9)),
  }
)]
#[case::date_time_in_utc(
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "shipDate": "2026-10-09T18:30:00Z" }),
  client::Order { pet_id: PET_ID, ship_date: Some(evening()), ..Default::default() }
)]
#[case::date_time_with_offset(
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "shipDate": "2026-10-09T20:30:00+02:00" }),
  client::Order { pet_id: PET_ID, ship_date: Some(evening()), ..Default::default() }
)]
#[case::date_time_with_fraction(
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "shipDate": "2026-10-09T18:30:00.25Z" }),
  client::Order { pet_id: PET_ID, ship_date: Some(evening() + TimeDelta::milliseconds(250)), ..Default::default() }
)]
#[case::int64_beyond_double_precision(
  json!({ "id": 9_007_199_254_740_993_i64, "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "status": "approved", "submittedAt": "2026-10-09T18:30:00Z" }),
  client::Adoption {
    id: 9_007_199_254_740_993,
    pet_id: PET_ID,
    adopter: None,
    status: client::AdoptionStatus::Approved,
    submitted_at: evening(),
  }
)]
#[case::byte(
  json!({ "type": "base64", "media_type": "image/png", "data": "QmlzY3VpdA==" }),
  client::Base64ImageSource { media_type: client::MediaType::ImagePng, data: b"Biscuit".to_vec(), ..Default::default() }
)]
#[case::time_and_byte_on_the_server(
  json!({
    "collarId": "COL-0A1B2C3D",
    "recordedAt": "2026-10-09T18:30:00Z",
    "recordedOn": "2026-10-09",
    "localTime": "08:15:30",
    "location": [45.52, -122.68],
    "samples": [72],
    "signature": "AAEC/w==",
    "sequence": 9_223_372_036_854_775_807_i64,
  }),
  server::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: evening(),
    recorded_on: Some(day(2026, 10, 9)),
    local_time: NaiveTime::from_hms_opt(8, 15, 30),
    location: vec![45.52, -122.68],
    samples: vec![72],
    signature: Some(vec![0, 1, 2, 255]),
    sequence: Some(i64::MAX),
    ..Default::default()
  }
)]
fn string_and_number_formats_decode<T: DeserializeOwned + PartialEq + Debug>(#[case] wire: Value, #[case] expected: T) {
  assert_eq!(decode::<T>(&wire), expected);
}

#[rstest]
#[case::date_time_uuid_and_int64(
  client::Order {
    id: Some(i64::MAX),
    pet_id: PET_ID,
    quantity: Some(2),
    ship_date: Some(evening() + TimeDelta::milliseconds(250)),
    status: Some(client::OrderStatus::Approved),
    complete: Some(true),
  },
  json!({
    "id": 9_223_372_036_854_775_807_i64,
    "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc",
    "quantity": 2,
    "shipDate": "2026-10-09T18:30:00.250Z",
    "status": "approved",
    "complete": true,
  })
)]
#[case::date(
  server::VaccineRecord {
    vaccine: "rabies".to_string(),
    date_administered: day(2026, 6, 9),
    veterinarian: None,
    next_due: Some(day(2027, 6, 9)),
  },
  json!({ "vaccine": "rabies", "date_administered": "2026-06-09", "nextDue": "2027-06-09" })
)]
#[case::byte(
  client::Base64ImageSource { media_type: client::MediaType::ImagePng, data: b"Biscuit".to_vec(), ..Default::default() },
  json!({ "type": "base64", "media_type": "image/png", "data": "QmlzY3VpdA==" })
)]
#[case::time_byte_and_uuid(
  client::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: evening(),
    recorded_on: Some(day(2026, 10, 9)),
    local_time: NaiveTime::from_hms_opt(8, 15, 30),
    location: vec![45.52, -122.68],
    samples: vec![72],
    reading_id: Some(PET_ID),
    signature: Some(vec![0, 1, 2, 255]),
    mode: None,
    ..Default::default()
  },
  json!({
    "collarId": "COL-0A1B2C3D",
    "recordedAt": "2026-10-09T18:30:00Z",
    "recordedOn": "2026-10-09",
    "localTime": "08:15:30",
    "location": [45.52, -122.68],
    "samples": [72],
    "readingId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc",
    "signature": "AAEC/w==",
  })
)]
fn string_and_number_formats_encode_canonically<T: Serialize>(#[case] value: T, #[case] wire: Value) {
  assert_eq!(encode(&value), wire);
}

#[rstest]
#[case::uuid(
  serde_json::from_value::<client::Lineage>,
  json!({ "petId": "not-a-uuid", "name": "Rex" })
)]
#[case::date(
  serde_json::from_value::<client::VaccineRecord>,
  json!({ "vaccine": "rabies", "date_administered": "06/09/2026" })
)]
#[case::date_time_without_offset(
  serde_json::from_value::<client::Order>,
  json!({ "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "shipDate": "2026-10-09T18:30:00" })
)]
#[case::time(
  serde_json::from_value::<server::CollarTelemetry>,
  json!({ "collarId": "COL-0A1B2C3D", "recordedAt": "2026-10-09T18:30:00Z", "localTime": "8am", "location": [0.0, 0.0], "samples": [1] })
)]
#[case::byte(
  serde_json::from_value::<client::Base64ImageSource>,
  json!({ "type": "base64", "media_type": "image/png", "data": "not base64!" })
)]
#[case::int64_from_a_fraction(
  serde_json::from_value::<client::Order>,
  json!({ "id": 1.5, "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc" })
)]
#[case::int64_from_a_string(
  serde_json::from_value::<client::Order>,
  json!({ "id": "42", "petId": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc" })
)]
#[case::double_from_a_string(
  serde_json::from_value::<client::Pet>,
  json!({ "id": "8f3c2a4e-1b2c-4d5e-8f90-123456789abc", "name": "Rex", "species": "dog", "weightKg": "12.5" })
)]
fn malformed_formats_are_rejected<T: Debug>(#[case] decode: fn(Value) -> serde_json::Result<T>, #[case] wire: Value) {
  assert!(decode(wire.clone()).is_err(), "{wire} decoded");
}

#[rstest]
#[case::problem_on_the_client(
  json!({ "type": "https://petstore.example/problems/out-of-stock", "title": "Out of stock", "status": 409, "traceId": "abc", "retryAfter": 30 }),
  client::Problem {
    r#type: Some("https://petstore.example/problems/out-of-stock".to_string()),
    title: Some("Out of stock".to_string()),
    status: Some(409),
    detail: None,
    instance: None,
    additional_properties: object(json!({ "traceId": "abc", "retryAfter": 30 })),
  }
)]
#[case::problem_on_the_server(
  json!({ "type": "about:blank", "status": 410, "errors": [{ "pointer": "/name" }] }),
  server::Problem {
    r#type: Some("about:blank".to_string()),
    title: None,
    status: Some(410),
    detail: None,
    instance: None,
    additional_properties: object(json!({ "errors": [{ "pointer": "/name" }] })),
  }
)]
#[case::recipe_list(
  json!({ "type": "object", "ingredients": { "flour": "2 cups" }, "required": ["flour"], "servings": 4, "tags": ["easy"] }),
  client::RecipeList {
    ingredients: Some(object(json!({ "flour": "2 cups" }))),
    required: Some(vec!["flour".to_string()]),
    r#type: "object".to_string(),
    additional_properties: object(json!({ "servings": 4, "tags": ["easy"] })),
  }
)]
#[case::recipe_list_with_null_members(
  json!({ "type": "object", "ingredients": null, "required": null, "servings": 1 }),
  server::RecipeList {
    ingredients: None,
    required: None,
    r#type: "object".to_string(),
    additional_properties: object(json!({ "servings": 1 })),
  }
)]
fn open_objects_keep_members_the_schema_does_not_name<T: Serialize + DeserializeOwned + PartialEq + Debug>(
  #[case] wire: Value,
  #[case] expected: T,
) {
  let decoded = decode::<T>(&wire);

  assert_eq!(decoded, expected);
  assert_eq!(
    encode(&decoded),
    without_null_members(wire),
    "re-encoding keeps every member"
  );
}

/// `value` with its null members removed: a nullable property that is null and one that is absent mean the same thing.
fn without_null_members(value: Value) -> Value {
  let Value::Object(members) = value else {
    return value;
  };
  Value::Object(members.into_iter().filter(|(_, member)| !member.is_null()).collect())
}

#[rstest]
#[case::unknown_member(json!({ "name": "Rex", "color": "brown" }), Some("unknown field `color`"))]
#[case::misspelled_member(json!({ "weight_kg": 12.0 }), Some("unknown field `weight_kg`"))]
#[case::known_members(json!({ "name": "Rex", "nickname": null, "weightKg": 12.0, "tags": ["calm"] }), None)]
fn closed_objects_reject_members_the_schema_does_not_name(#[case] wire: Value, #[case] rejection: Option<&str>) {
  let decoded = serde_json::from_value::<server::PetPatch>(wire.clone());
  match (decoded, rejection) {
    (Err(error), Some(message)) => assert!(error.to_string().contains(message), "{wire}: {error}"),
    (Ok(_), None) => {}
    (decoded, rejection) => panic!("{wire}: expected rejection {rejection:?}, got {decoded:?}"),
  }
}

/// The deprecated `tag` member is part of the response schema, so it is populated on both sides.
#[expect(deprecated)]
fn pet_with_every_member() -> (server::Pet, client::Pet) {
  let vaccination = |vaccine: &str| (vaccine.to_string(), day(2026, 1, 15), Some(day(2027, 1, 15)));
  let (vaccine, administered, next_due) = vaccination("rabies");
  (
    server::Pet {
      id: PET_ID,
      name: "Biscuit".to_string(),
      species: server::Species::Dog,
      status: Some(server::PetStatus::Pending),
      nickname: Some("Biscuit the Brave".to_string()),
      birth_date: Some(day(2021, 4, 1)),
      weight_kg: Some(12.5),
      microchip: Some("123456789012345".to_string()),
      tags: Some(IndexSet::from(["calm".to_string(), "house-trained".to_string()])),
      photo_urls: Some(vec!["https://cdn.example/biscuit.png".to_string()]),
      allergies: Some(Box::new(server::Health::allergies(Some(vec!["pollen".to_string()])))),
      vaccinations: Some(IndexMap::from([(
        "core".to_string(),
        vec![server::VaccineRecord {
          vaccine: vaccine.clone(),
          date_administered: administered,
          veterinarian: Some("Dr. Reyes".to_string()),
          next_due,
        }],
      )])),
      created_at: Some(evening()),
      tag: Some("legacy".to_string()),
    },
    client::Pet {
      id: PET_ID,
      name: "Biscuit".to_string(),
      species: client::Species::Dog,
      status: Some(client::PetStatus::Pending),
      nickname: Some("Biscuit the Brave".to_string()),
      birth_date: Some(day(2021, 4, 1)),
      weight_kg: Some(12.5),
      microchip: Some("123456789012345".to_string()),
      tags: Some(IndexSet::from(["calm".to_string(), "house-trained".to_string()])),
      photo_urls: Some(vec!["https://cdn.example/biscuit.png".to_string()]),
      allergies: Some(Box::new(client::Health::allergies(Some(vec!["pollen".to_string()])))),
      vaccinations: Some(IndexMap::from([(
        "core".to_string(),
        vec![client::VaccineRecord {
          vaccine,
          date_administered: administered,
          veterinarian: Some("Dr. Reyes".to_string()),
          next_due,
        }],
      )])),
      created_at: Some(evening()),
      tag: Some("legacy".to_string()),
    },
  )
}

fn cat() -> (server::Cat, client::Cat) {
  (
    server::Cat {
      id: PET_ID,
      name: "Miso".to_string(),
      species: server::Species::Cat,
      allergies: Some(Box::new(server::Health::diet(Some("salmon".to_string())))),
      lives: 9,
      favorite_toy: Some("feather".to_string()),
      ..Default::default()
    },
    client::Cat {
      id: PET_ID,
      name: "Miso".to_string(),
      species: client::Species::Cat,
      status: None,
      allergies: Some(Box::new(client::Health::diet(Some("salmon".to_string())))),
      lives: 9,
      favorite_toy: Some("feather".to_string()),
      ..Default::default()
    },
  )
}

fn lineage() -> (server::Lineage, client::Lineage) {
  let server_parent = |name: &str| server::Lineage {
    pet_id: Uuid::from_u128(7),
    name: name.to_string(),
    species: None,
    parents: Some(vec![]),
  };
  let client_parent = |name: &str| client::Lineage {
    pet_id: Uuid::from_u128(7),
    name: name.to_string(),
    species: None,
    parents: Some(vec![]),
  };
  (
    server::Lineage {
      pet_id: PET_ID,
      name: "Biscuit".to_string(),
      species: Some(server::Species::Dog),
      parents: Some(vec![server_parent("sire"), server_parent("dam")]),
    },
    client::Lineage {
      pet_id: PET_ID,
      name: "Biscuit".to_string(),
      species: Some(client::Species::Dog),
      parents: Some(vec![client_parent("sire"), client_parent("dam")]),
    },
  )
}

fn adoption() -> (server::Adoption, client::Adoption) {
  (
    server::Adoption {
      id: 9_007_199_254_740_993,
      pet_id: PET_ID,
      adopter: Some(server::Adopter {
        name: "Ada".to_string(),
        email: "ada@example.com".to_string(),
        phone: Some("+15035550100".to_string()),
        address: Some(server::Address {
          city: "Portland".to_string(),
          zip: None,
        }),
      }),
      status: server::AdoptionStatus::UnderReview,
      submitted_at: evening(),
    },
    client::Adoption {
      id: 9_007_199_254_740_993,
      pet_id: PET_ID,
      adopter: Some(client::Adopter {
        name: "Ada".to_string(),
        email: "ada@example.com".to_string(),
        phone: Some("+15035550100".to_string()),
        address: Some(client::Address {
          city: "Portland".to_string(),
          zip: None,
        }),
      }),
      status: client::AdoptionStatus::UnderReview,
      submitted_at: evening(),
    },
  )
}

fn search_results() -> (server::SearchResults, client::SearchResults) {
  let (server_pet, client_pet) = pet_with_every_member();
  (
    server::SearchResults {
      items: vec![server_pet],
      total: 1,
      next_cursor: Some("c2".to_string()),
    },
    client::SearchResults {
      items: vec![client_pet],
      total: 1,
      next_cursor: Some("c2".to_string()),
    },
  )
}

fn problem() -> (server::Problem, client::Problem) {
  let extensions = object(json!({ "traceId": "abc", "retryAfter": 30 }));
  (
    server::Problem {
      r#type: None,
      title: Some("Conflict".to_string()),
      status: Some(409),
      detail: Some("Biscuit is not available".to_string()),
      instance: Some("/pets/1".to_string()),
      additional_properties: extensions.clone(),
    },
    client::Problem {
      r#type: Some("about:blank".to_string()),
      title: Some("Conflict".to_string()),
      status: Some(409),
      detail: Some("Biscuit is not available".to_string()),
      instance: Some("/pets/1".to_string()),
      additional_properties: extensions,
    },
  )
}

fn content_response() -> (server::ContentResponse, client::ContentResponse) {
  (
    server::ContentResponse {
      id: "msg_7".to_string(),
      content: Box::new(server::ContentBlock::ToolResult(server::ToolResultBlock {
        tool_use_id: "toolu_1".to_string(),
        content: server::ToolResultContent::Array(vec![
          server::ToolResultContentBlock::text("Two vets nearby".to_string()),
          server::ToolResultContentBlock::Image(server::ImageBlock {
            source: Box::new(server::ImageSource::Base64(server::Base64ImageSource {
              media_type: server::MediaType::ImageJpeg,
              data: vec![0xff, 0xd8, 0xff],
              ..Default::default()
            })),
            alt_text: Some("map".to_string()),
            ..Default::default()
          }),
        ]),
        iterations: Some(vec![server::UsageCounterUsageCounterKind::a(3)]),
        is_error: Some(false),
        ..Default::default()
      })),
      usage: Some(server::Usage {
        input_tokens: 10,
        output_tokens: 20,
      }),
    },
    client::ContentResponse {
      id: "msg_7".to_string(),
      content: Box::new(client::ContentBlock::ToolResult(client::ToolResultBlock {
        tool_use_id: "toolu_1".to_string(),
        content: client::ToolResultContent::Array(vec![
          client::ToolResultContentBlock::text("Two vets nearby".to_string()),
          client::ToolResultContentBlock::Image(client::ImageBlock {
            source: Box::new(client::ImageSource::Base64(client::Base64ImageSource {
              media_type: client::MediaType::ImageJpeg,
              data: vec![0xff, 0xd8, 0xff],
              ..Default::default()
            })),
            alt_text: Some("map".to_string()),
            ..Default::default()
          }),
        ]),
        iterations: Some(vec![client::UsageCounterUsageCounterKind::a(3)]),
        is_error: Some(false),
        ..Default::default()
      })),
      usage: Some(client::Usage {
        input_tokens: 10,
        output_tokens: 20,
      }),
    },
  )
}

fn assistant_events() -> (server::AssistantEventList, client::AssistantEventList) {
  (
    server::AssistantEventList {
      events: vec![
        server::AssistantEvent::message_start(server::Message {
          id: "msg_1".to_string(),
          role: server::Role::Assistant,
          content: vec![server::ContentBlock::code("ls".to_string())],
          model: "chat-1.1".to_string(),
          stop_reason: Some(server::StopReason::MaxTokens),
          usage: None,
        }),
        server::AssistantEvent::ContentBlockStart(server::ContentBlockStartEvent {
          index: 0,
          content_block: Box::new(server::ContentBlock::text("Hi".to_string())),
          ..Default::default()
        }),
        server::AssistantEvent::ContentBlockDelta(server::ContentBlockDeltaEvent {
          index: 0,
          delta: Box::new(server::Delta::input_json("{}".to_string())),
          ..Default::default()
        }),
        server::AssistantEvent::content_block_stop(0),
        server::AssistantEvent::ping(),
        server::AssistantEvent::message_stop(),
      ],
    },
    client::AssistantEventList {
      events: vec![
        client::AssistantEvent::message_start(client::Message {
          id: "msg_1".to_string(),
          role: client::Role::Assistant,
          content: vec![client::ContentBlock::code("ls".to_string())],
          model: "chat-1.1".to_string(),
          stop_reason: Some(client::StopReason::MaxTokens),
          usage: None,
        }),
        client::AssistantEvent::ContentBlockStart(client::ContentBlockStartEvent {
          index: 0,
          content_block: Box::new(client::ContentBlock::text("Hi".to_string())),
          ..Default::default()
        }),
        client::AssistantEvent::ContentBlockDelta(client::ContentBlockDeltaEvent {
          index: 0,
          delta: Box::new(client::Delta::input_json("{}".to_string())),
          ..Default::default()
        }),
        client::AssistantEvent::content_block_stop(0),
        client::AssistantEvent::ping(),
        client::AssistantEvent::message_stop(),
      ],
    },
  )
}

fn chat_preference() -> (server::ChatPreference, client::ChatPreference) {
  (
    server::ChatPreference {
      model: server::ChatModelId::Other("chat-9-preview".to_string()),
      object: Some("chat.preference".to_string()),
      fallback: Some(server::ChatModel::Chat10),
      tier: Some(server::ChatPreferenceTier::Priority),
    },
    client::ChatPreference {
      model: client::ChatModelId::Other("chat-9-preview".to_string()),
      object: Some("chat.preference".to_string()),
      fallback: Some(client::ChatModel::Chat10),
      tier: Some(client::ChatPreferenceTier::Priority),
    },
  )
}

fn vitals_reading() -> (server::VitalsReading, client::VitalsReading) {
  (
    server::VitalsReading {
      id: "r1".to_string(),
      heart_rate_bpm: 88,
      temperature_c: Some(38.6),
      activity: Some(server::VitalsReadingActivity::Running),
      timestamp: Some(evening()),
    },
    client::VitalsReading {
      id: "r1".to_string(),
      heart_rate_bpm: 88,
      temperature_c: Some(38.6),
      activity: Some(client::VitalsReadingActivity::Running),
      timestamp: Some(evening()),
    },
  )
}

fn feed_event() -> (server::AdoptionFeedEvent, client::AdoptionFeedEvent) {
  (
    server::AdoptionFeedEvent {
      r#type: server::AdoptionFeedEventType::Updated,
      payload: server::AdoptionFeedPayload {
        id: "42".to_string(),
        name: None,
        value: Some(-1),
      },
    },
    client::AdoptionFeedEvent {
      r#type: client::AdoptionFeedEventType::Updated,
      payload: client::AdoptionFeedPayload {
        id: "42".to_string(),
        name: None,
        value: Some(-1),
      },
    },
  )
}

fn error_response() -> (server::ErrorResponse, client::ErrorResponse) {
  (
    server::ErrorResponse {
      r#type: "error".to_string(),
      error: server::ErrorDetails {
        r#type: server::ErrorType::PermissionError,
        message: "staff only".to_string(),
      },
    },
    client::ErrorResponse {
      r#type: "error".to_string(),
      error: client::ErrorDetails {
        r#type: client::ErrorType::PermissionError,
        message: "staff only".to_string(),
      },
    },
  )
}

#[rstest]
#[case::pet(pet_with_every_member())]
#[case::cat(cat())]
#[case::lineage(lineage())]
#[case::adoption(adoption())]
#[case::search_results(search_results())]
#[case::problem(problem())]
#[case::content_response(content_response())]
#[case::assistant_events(assistant_events())]
#[case::chat_preference(chat_preference())]
#[case::vitals_reading(vitals_reading())]
#[case::feed_event(feed_event())]
#[case::error_response(error_response())]
fn what_the_server_writes_the_client_reads_back<S: Serialize, C: DeserializeOwned + PartialEq + Debug>(
  #[case] (sent, expected): (S, C),
) {
  assert_eq!(decode::<C>(&encode(&sent)), expected);
}

fn property_config() -> ProptestConfig {
  ProptestConfig {
    failure_persistence: None,
    ..ProptestConfig::with_cases(64)
  }
}

fn uuid() -> impl Strategy<Value = Uuid> {
  any::<u128>().prop_map(Uuid::from_u128)
}

fn date() -> impl Strategy<Value = NaiveDate> {
  (693_960i32..767_000).prop_map(|days| NaiveDate::from_num_days_from_ce_opt(days).expect("an in-range day"))
}

fn instant() -> impl Strategy<Value = DateTime<Utc>> {
  (0i64..4_102_444_800, 0u32..1_000_000_000)
    .prop_map(|(seconds, nanos)| DateTime::from_timestamp(seconds, nanos).expect("an in-range instant"))
}

fn web_url() -> impl Strategy<Value = String> {
  "https://[a-z]{1,12}\\.example/[a-z0-9]{0,12}"
}

fn species() -> impl Strategy<Value = (server::Species, client::Species)> {
  select(vec![
    (server::Species::Dog, client::Species::Dog),
    (server::Species::Cat, client::Species::Cat),
    (server::Species::Bird, client::Species::Bird),
    (server::Species::Fish, client::Species::Fish),
    (server::Species::Reptile, client::Species::Reptile),
    (server::Species::SmallMammal, client::Species::SmallMammal),
  ])
}

fn pet_status() -> impl Strategy<Value = (server::PetStatus, client::PetStatus)> {
  select(vec![
    (server::PetStatus::Available, client::PetStatus::Available),
    (server::PetStatus::Pending, client::PetStatus::Pending),
    (server::PetStatus::Adopted, client::PetStatus::Adopted),
  ])
}

fn adoption_status() -> impl Strategy<Value = (client::AdoptionStatus, server::AdoptionStatus)> {
  select(vec![
    (client::AdoptionStatus::Submitted, server::AdoptionStatus::Submitted),
    (client::AdoptionStatus::UnderReview, server::AdoptionStatus::UnderReview),
    (client::AdoptionStatus::Approved, server::AdoptionStatus::Approved),
    (client::AdoptionStatus::Rejected, server::AdoptionStatus::Rejected),
    (client::AdoptionStatus::Completed, server::AdoptionStatus::Completed),
  ])
}

fn order_status() -> impl Strategy<Value = (server::OrderStatus, client::OrderStatus)> {
  select(vec![
    (server::OrderStatus::Placed, client::OrderStatus::Placed),
    (server::OrderStatus::Approved, client::OrderStatus::Approved),
    (server::OrderStatus::Delivered, client::OrderStatus::Delivered),
  ])
}

fn health() -> impl Strategy<Value = (server::Health, client::Health)> {
  prop_oneof![
    option::of(collection::vec("[a-z]{1,10}", 0..3)).prop_map(|allergens| (
      server::Health::allergies(allergens.clone()),
      client::Health::allergies(allergens)
    )),
    option::of("\\PC{1,12}").prop_map(|food| (server::Health::diet(food.clone()), client::Health::diet(food))),
  ]
}

prop_compose! {
  fn vaccine_record()(
    vaccine in "\\PC{1,16}",
    date_administered in date(),
    veterinarian in option::of("\\PC{1,16}"),
    next_due in option::of(date()),
  ) -> (server::VaccineRecord, client::VaccineRecord) {
    (
      server::VaccineRecord { vaccine: vaccine.clone(), date_administered, veterinarian: veterinarian.clone(), next_due },
      client::VaccineRecord { vaccine, date_administered, veterinarian, next_due },
    )
  }
}

type VaccinationsPair = (
  IndexMap<String, Vec<server::VaccineRecord>>,
  IndexMap<String, Vec<client::VaccineRecord>>,
);

fn vaccinations() -> impl Strategy<Value = VaccinationsPair> {
  collection::vec(("[a-z]{1,8}", collection::vec(vaccine_record(), 0..3)), 0..3).prop_map(|families| {
    families
      .into_iter()
      .map(|(family, records)| {
        let (sent, expected) = records.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
        ((family.clone(), sent), (family, expected))
      })
      .unzip()
  })
}

prop_compose! {
  #[expect(deprecated)]
  fn pet()(
    (id, name, (server_species, client_species), status, nickname, birth_date, weight_kg) in (
      uuid(),
      "\\PC{1,64}",
      species(),
      option::of(pet_status()),
      option::of("\\PC{0,16}"),
      option::of(date()),
      option::of(0.01f64..500.0),
    ),
    (microchip, tags, photo_urls, allergies, vaccinations, created_at, tag) in (
      option::of("[0-9]{15}"),
      option::of(collection::vec("[a-z]{1,8}", 0..10)),
      option::of(collection::vec(web_url(), 0..3)),
      option::of(health()),
      option::of(vaccinations()),
      option::of(instant()),
      option::of("[a-z]{1,8}"),
    ),
  ) -> (server::Pet, client::Pet) {
    let (server_status, client_status) = status.unzip();
    let (server_allergies, client_allergies) = allergies.unzip();
    let (server_vaccinations, client_vaccinations) = vaccinations.unzip();
    let tags = tags.map(|tags| tags.into_iter().collect::<IndexSet<_>>());
    (
      server::Pet {
        id,
        name: name.clone(),
        species: server_species,
        status: server_status,
        nickname: nickname.clone(),
        birth_date,
        weight_kg,
        microchip: microchip.clone(),
        tags: tags.clone(),
        photo_urls: photo_urls.clone(),
        allergies: server_allergies.map(Box::new),
        vaccinations: server_vaccinations,
        created_at,
        tag: tag.clone(),
      },
      client::Pet {
        id,
        name,
        species: client_species,
        status: client_status,
        nickname,
        birth_date,
        weight_kg,
        microchip,
        tags,
        photo_urls,
        allergies: client_allergies.map(Box::new),
        vaccinations: client_vaccinations,
        created_at,
        tag,
      },
    )
  }
}

prop_compose! {
  fn order()(
    id in option::of(any::<i64>()),
    pet_id in uuid(),
    quantity in option::of(1i32..=10),
    ship_date in option::of(instant()),
    status in option::of(order_status()),
    complete in option::of(any::<bool>()),
  ) -> (server::Order, client::Order) {
    let (server_status, client_status) = status.unzip();
    (
      server::Order { id, pet_id, quantity, ship_date, status: server_status, complete },
      client::Order {
        id,
        pet_id,
        quantity: quantity.or(Some(1)),
        ship_date,
        status: client_status,
        complete: complete.or(Some(false)),
      },
    )
  }
}

fn inventory() -> impl Strategy<Value = (server::Inventory, client::Inventory)> {
  collection::vec(("[a-z_]{1,12}", 0i32..=i32::MAX), 0..6).prop_map(|counts| {
    let counts = counts.into_iter().collect::<IndexMap<_, _>>();
    (
      server::Inventory {
        additional_properties: counts.clone(),
      },
      client::Inventory {
        additional_properties: counts,
      },
    )
  })
}

prop_compose! {
  fn adoption_event()(
    adoption_id in any::<i64>(),
    pet_id in option::of(uuid()),
    (client_status, server_status) in adoption_status(),
    occurred_at in instant(),
  ) -> (client::AdoptionEvent, server::AdoptionEvent) {
    (
      client::AdoptionEvent { adoption_id, pet_id, status: client_status, occurred_at },
      server::AdoptionEvent { adoption_id, pet_id, status: server_status, occurred_at },
    )
  }
}

proptest! {
  #![proptest_config(property_config())]

  #[test]
  fn pets_written_by_the_server_read_back_on_the_client((sent, expected) in pet()) {
    prop_assert_eq!(serde_json::from_value::<client::Pet>(serde_json::to_value(&sent)?)?, expected);
  }

  #[test]
  fn orders_written_by_the_server_read_back_on_the_client((sent, expected) in order()) {
    prop_assert_eq!(serde_json::from_value::<client::Order>(serde_json::to_value(&sent)?)?, expected);
  }

  #[test]
  fn vaccine_records_written_by_the_server_read_back_on_the_client((sent, expected) in vaccine_record()) {
    prop_assert_eq!(serde_json::from_value::<client::VaccineRecord>(serde_json::to_value(&sent)?)?, expected);
  }

  #[test]
  fn inventories_written_by_the_server_read_back_on_the_client((sent, expected) in inventory()) {
    prop_assert_eq!(serde_json::from_value::<client::Inventory>(serde_json::to_value(&sent)?)?, expected);
  }

  /// `AdoptionEvent` is the `petAdopted` webhook body: the client writes it and the server reads it.
  #[test]
  fn adoption_events_written_by_the_client_read_back_on_the_server((sent, expected) in adoption_event()) {
    prop_assert_eq!(serde_json::from_value::<server::AdoptionEvent>(serde_json::to_value(&sent)?)?, expected);
  }
}
