use std::collections::BTreeSet;

use chrono::NaiveDate;
use futures::StreamExt as _;
use indexmap::IndexMap;
use petstore_client::{self as client, ApiResponse, PetstoreClient, WithHeaders};
use rstest::rstest;

use crate::support::{
  API_KEY, API_VERSION, COMPATIBILITY_DATE, Petstore, QUERY_KEY, SESSION, STAFF_TOKEN, now, petstore,
};

const OPERATIONS: [&str; 63] = [
  "AdoptPet",
  "AdoptPetStatusChanged",
  "CreateApiKey",
  "CreatePet",
  "DeleteOrder",
  "DeletePet",
  "DescribePets",
  "EchoResponseQuality",
  "GetAdoption",
  "GetApiKey",
  "GetChatPreference",
  "GetHealth",
  "GetInventory",
  "GetMediaAccepted",
  "GetMediaBoth",
  "GetMediaKind",
  "GetMediaLatin",
  "GetMediaNickname",
  "GetMediaNullableName",
  "GetMediaPetText",
  "GetMediaText",
  "GetMediaWildcard",
  "GetOrderById",
  "GetPetCard",
  "GetPetLineage",
  "GetPetPhoto",
  "InventoryLow",
  "ListApiKeys",
  "ListAssistantEvents",
  "ListAuditEvents",
  "ListCarriers",
  "ListCats",
  "ListPets",
  "ListVaccinations",
  "PetAdopted",
  "PetExists",
  "PlaceOrder",
  "PostMediaUnion",
  "PutMediaRaw",
  "RecordVaccination",
  "ReplaceApiKey",
  "ReplacePetPhoto",
  "RevokeApiKey",
  "SearchPets",
  "SendAssistantMessage",
  "SetPetNotes",
  "ShowPetById",
  "StreamAdoptionFeed",
  "StreamAssistantMessage",
  "StreamAssistantTokens",
  "StreamPetVitals",
  "SubmitCollarTelemetry",
  "SubmitIntakeForm",
  "SubmitMediaLabels",
  "SubmitTallies",
  "SubscribeNewsletter",
  "TraceHealth",
  "UnsubscribeNewsletter",
  "UpdateAssistantSettings",
  "UpdateChatPreference",
  "UpdatePet",
  "UploadMedia",
  "UploadPetImage",
];

fn staff_client(petstore: &Petstore) -> anyhow::Result<PetstoreClient> {
  Ok(
    PetstoreClient::with_base_url(&petstore.base_url)?
      .with_api_key_auth(API_KEY)
      .with_bearer_auth(STAFF_TOKEN)
      .with_query_key(QUERY_KEY)
      .with_session_cookie(SESSION),
  )
}

async fn pets_operations(petstore: &Petstore) -> anyhow::Result<client::Pet> {
  let pets = &petstore.client;
  let anonymous = petstore.anonymous();

  answered!(anonymous.get_health(client::GetHealthRequest {}).await?, ApiResponse::NoContent(()) => ());
  let trace = answered!(anonymous.trace_health(client::TraceHealthRequest {}).await?, ApiResponse::Ok(trace) => trace);
  anyhow::ensure!(trace == "TRACE /health", "{trace}");
  let allow = answered!(
    anonymous.describe_pets(client::DescribePetsRequest {}).await?,
    ApiResponse::NoContent(WithHeaders { headers, .. }) => headers.allow
  );
  anyhow::ensure!(allow == "GET, POST, OPTIONS", "{allow}");

  let new_pet = client::NewPet {
    name: "Biscuit".to_string(),
    species: client::Species::Dog,
    tags: Some(["good-boy".to_string()].into_iter().collect()),
    ..Default::default()
  };
  let (location, pet) = answered!(
    pets.create_pet(client::CreatePetRequest::builder().body(new_pet).build()?).await?,
    ApiResponse::Created(WithHeaders { headers, body }) => (headers.location, body)
  );
  anyhow::ensure!(location == format!("/pets/{}", pet.id), "{location}");
  let pet_id = pet.id;

  let quality = client::EchoResponseQualityRequest::builder()
    .quality(client::ResponseQuality::Value1)
    .boost(client::ResponseQuality::True)
    .build()?;
  let echoed = answered!(pets.echo_response_quality(quality).await?, ApiResponse::Ok(echoed) => echoed);
  anyhow::ensure!(echoed == client::ResponseQuality::True, "{echoed:?}");

  let list = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .build()?;
  let listed = answered!(pets.list_pets(list).await?, ApiResponse::Ok(WithHeaders { body, .. }) => body);
  anyhow::ensure!(listed.len() == 1, "{listed:?}");
  let search = client::SearchPetsRequest::builder().q("bisc".to_string()).build()?;
  let results = answered!(pets.search_pets(search).await?, ApiResponse::Ok(results) => results);
  anyhow::ensure!(results.total == 1, "{results:?}");

  let show = client::ShowPetByIdRequest::builder()
    .pet_id(pet_id)
    .x_api_version(API_VERSION.to_string())
    .build()?;
  let shown = answered!(pets.show_pet_by_id(show).await?, ApiResponse::Ok(WithHeaders { body, .. }) => body);
  anyhow::ensure!(shown == pet, "{shown:?}");
  let exists = client::PetExistsRequest::builder().pet_id(pet_id).build()?;
  answered!(pets.pet_exists(exists).await?, ApiResponse::Ok(WithHeaders { .. }) => ());

  let patch = client::PetPatch {
    nickname: Some("Biscuit Jr.".to_string()),
    ..Default::default()
  };
  let update = client::UpdatePetRequest::builder().pet_id(pet_id).body(patch).build()?;
  let updated = answered!(pets.update_pet(update).await?, ApiResponse::Ok(updated) => updated);
  anyhow::ensure!(updated.nickname.as_deref() == Some("Biscuit Jr."), "{updated:?}");

  pet_records(petstore, pet_id).await?;
  Ok(pet)
}

async fn pet_records(petstore: &Petstore, pet_id: uuid::Uuid) -> anyhow::Result<()> {
  let pets = &petstore.client;
  let image = client::ImageRequestBody {
    name: "Sir Biscuit".to_string(),
    image: vec![0x89, b'P', b'N', b'G'],
  };
  let upload = client::UploadPetImageRequest::builder()
    .pet_id(pet_id)
    .body(image)
    .build()?;
  let renamed = answered!(pets.upload_pet_image(upload).await?, ApiResponse::Ok(renamed) => renamed);
  anyhow::ensure!(renamed.name == "Sir Biscuit", "{renamed:?}");

  let replace = client::ReplacePetPhotoRequest::builder()
    .pet_id(pet_id)
    .body(vec![1, 2, 3])
    .build()?;
  answered!(pets.replace_pet_photo(replace).await?, ApiResponse::NoContent(()) => ());
  let photo = client::GetPetPhotoRequest::builder().pet_id(pet_id).build()?;
  let photo = answered!(pets.get_pet_photo(photo).await?, ApiResponse::Ok(photo) => photo);
  anyhow::ensure!(photo == [1, 2, 3], "{photo:?}");

  let notes = client::SetPetNotesRequest::builder()
    .pet_id(pet_id)
    .body("Naps after lunch.".to_string())
    .build()?;
  answered!(pets.set_pet_notes(notes).await?, ApiResponse::NoContent(()) => ());
  let card = client::GetPetCardRequest::builder().pet_id(pet_id).build()?;
  let card = answered!(pets.get_pet_card(card).await?, ApiResponse::Ok(card) => card);
  anyhow::ensure!(card.ends_with("Notes: Naps after lunch."), "{card}");

  let lineage = client::GetPetLineageRequest::builder()
    .pet_id(pet_id)
    .depth(1)
    .build()?;
  let lineage = answered!(pets.get_pet_lineage(lineage).await?, ApiResponse::Ok(lineage) => lineage);
  anyhow::ensure!(lineage.parents.as_ref().map(Vec::len) == Some(2), "{lineage:?}");

  let vaccine = client::VaccineRecord {
    vaccine: "rabies".to_string(),
    date_administered: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap_or_default(),
    ..Default::default()
  };
  let record = client::RecordVaccinationRequest::builder()
    .pet_id(pet_id)
    .body(vaccine.clone())
    .build()?;
  let recorded = answered!(pets.record_vaccination(record).await?, ApiResponse::Created(recorded) => recorded);
  anyhow::ensure!(recorded == vaccine, "{recorded:?}");
  let vaccinations = client::ListVaccinationsRequest::builder().pet_id(pet_id).build()?;
  let vaccinations = answered!(pets.list_vaccinations(vaccinations).await?, ApiResponse::Ok(list) => list);
  anyhow::ensure!(vaccinations == [vaccine], "{vaccinations:?}");

  petstore.service.with_state(|state| {
    state.vitals = vec![petstore_server::VitalsReading {
      id: "reading-1".to_string(),
      heart_rate_bpm: 82,
      timestamp: Some(now()),
      ..Default::default()
    }];
  });
  let vitals = client::StreamPetVitalsRequest::builder().pet_id(pet_id).build()?;
  let vitals = answered!(pets.stream_pet_vitals(vitals).await?, ApiResponse::Ok(vitals) => vitals);
  anyhow::ensure!(vitals.count().await == 1);

  #[expect(deprecated, reason = "listCats is deprecated in the document")]
  let cats = pets.list_cats(client::ListCatsRequest::builder().build()?).await?;
  let cats = answered!(cats, ApiResponse::Ok(WithHeaders { body, .. }) => body);
  anyhow::ensure!(cats.is_empty(), "{cats:?}");
  let inventory =
    answered!(pets.get_inventory(client::GetInventoryRequest {}).await?, ApiResponse::Ok(inventory) => inventory);
  anyhow::ensure!(inventory.additional_properties.get("dog") == Some(&1), "{inventory:?}");
  Ok(())
}

async fn store_operations(petstore: &Petstore, pet_id: uuid::Uuid) -> anyhow::Result<i64> {
  let pets = &petstore.client;
  let application = client::AdoptionRequest {
    pet_id,
    adopter: client::Adopter {
      name: "Avery".to_string(),
      email: "avery@example.com".to_string(),
      ..Default::default()
    },
    callback_url: Some(petstore.url("/callbacks/adoptPet/statusChanged")),
    ..Default::default()
  };
  let adopt = client::AdoptPetRequest::builder().body(application).build()?;
  let (location, adoption) = answered!(
    pets.adopt_pet(adopt).await?,
    ApiResponse::Accepted(WithHeaders { headers, body }) => (headers.location, body)
  );
  anyhow::ensure!(location == format!("/adoptions/{}", adoption.id), "{location}");
  let fetch = client::GetAdoptionRequest::builder().adoption_id(adoption.id).build()?;
  let fetched = answered!(pets.get_adoption(fetch).await?, ApiResponse::Ok(fetched) => fetched);
  anyhow::ensure!(fetched == adoption, "{fetched:?}");
  let feed = client::StreamAdoptionFeedRequest::builder().build()?;
  let feed = answered!(pets.stream_adoption_feed(feed).await?, ApiResponse::Ok(feed) => feed);
  anyhow::ensure!(feed.count().await == 1);

  let telemetry = client::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: now(),
    location: vec![45.52, -122.68],
    samples: vec![72, 75, 80],
    ..Default::default()
  };
  let submit = client::SubmitCollarTelemetryRequest::builder()
    .collar_id("COL-0A1B2C3D".to_string())
    .body(telemetry)
    .build()?;
  let receipt = answered!(pets.submit_collar_telemetry(submit).await?, ApiResponse::Created(receipt) => receipt);
  anyhow::ensure!(receipt.reading_count == 3, "{receipt:?}");

  let order = client::Order {
    pet_id,
    ..Default::default()
  };
  let place = client::PlaceOrderRequest::builder().body(order).build()?;
  let order = answered!(pets.place_order(place).await?, ApiResponse::Ok(order) => order);
  let order_id = order
    .id
    .ok_or_else(|| anyhow::anyhow!("the server assigns an order id"))?;
  let fetch = client::GetOrderByIdRequest::builder().order_id(order_id).build()?;
  let fetched = answered!(pets.get_order_by_id(fetch).await?, ApiResponse::Ok(fetched) => fetched);
  anyhow::ensure!(fetched == order, "{fetched:?}");
  let cancel = client::DeleteOrderRequest::builder().order_id(order_id).build()?;
  answered!(pets.delete_order(cancel).await?, ApiResponse::NoContent(()) => ());
  let carriers =
    answered!(pets.list_carriers(client::ListCarriersRequest {}).await?, ApiResponse::Ok(carriers) => carriers);
  anyhow::ensure!(carriers.len() == 3, "{carriers:?}");

  let signup = client::NewsletterSignup {
    email: "avery@example.com".to_string(),
    frequency: client::Frequency::Monthly,
    ..Default::default()
  };
  let subscribe = client::SubscribeNewsletterRequest::builder().body(signup).build()?;
  let subscription = answered!(
    petstore.anonymous().subscribe_newsletter(subscribe).await?,
    ApiResponse::Created(subscription) => subscription
  );
  anyhow::ensure!(subscription.frequency == client::Frequency::Monthly, "{subscription:?}");
  let unsubscribe = client::UnsubscribeNewsletterRequest::builder().build()?;
  answered!(petstore.anonymous().unsubscribe_newsletter(unsubscribe).await?, ApiResponse::NoContent(()) => ());
  Ok(adoption.id)
}

async fn assistant_operations(pets: &PetstoreClient) -> anyhow::Result<()> {
  let message = client::ContentRequest {
    blocks: vec![client::ContentBlock::text("Is Biscuit due for shots?".to_string())],
    ..Default::default()
  };
  let send = client::SendAssistantMessageRequest::builder()
    .body(message.clone())
    .build()?;
  let reply = answered!(pets.send_assistant_message(send).await?, ApiResponse::Ok(reply) => reply);
  anyhow::ensure!(*reply.content == message.blocks[0], "{reply:?}");
  let stream = client::StreamAssistantMessageRequest::builder().body(message).build()?;
  answered!(
    pets.stream_assistant_message(stream).await?,
    ApiResponse::Ok(client::ContentResponseOrAssistantEventFrameStream::ContentResponse(_)) => ()
  );
  let tokens = answered!(
    pets.stream_assistant_tokens(client::StreamAssistantTokensRequest {}).await?,
    ApiResponse::Ok(tokens) => tokens
  );
  anyhow::ensure!(tokens.count().await == 0);
  let events = client::ListAssistantEventsRequest::builder()
    .conversation("conversation-1".to_string())
    .build()?;
  let events = answered!(pets.list_assistant_events(events).await?, ApiResponse::Ok(events) => events);
  anyhow::ensure!(!events.events.is_empty(), "{events:?}");
  let preference = answered!(
    pets.get_chat_preference(client::GetChatPreferenceRequest {}).await?,
    ApiResponse::Ok(preference) => preference
  );
  let update = client::UpdateChatPreferenceRequest::builder()
    .body(preference.clone())
    .build()?;
  let stored = answered!(pets.update_chat_preference(update).await?, ApiResponse::Ok(stored) => stored);
  anyhow::ensure!(stored == preference, "{stored:?}");
  let settings = client::AssistantSettings {
    sample_rate: Some(client::SampleRate::Value48000),
    ..Default::default()
  };
  let update = client::UpdateAssistantSettingsRequest::builder()
    .body(settings.clone())
    .build()?;
  let echoed = answered!(pets.update_assistant_settings(update).await?, ApiResponse::Ok(echoed) => echoed);
  anyhow::ensure!(echoed == settings, "{echoed:?}");
  Ok(())
}

async fn media_operations(pets: &PetstoreClient) -> anyhow::Result<()> {
  let form = client::IntakeForm {
    name: "Biscuit".to_string(),
    attachments: vec![b"vet-record".to_vec()],
    thumbnail: b"thumb".to_vec(),
    encoded: b"chip".to_vec(),
    count: 1,
    ..Default::default()
  };
  let intake = client::SubmitIntakeFormRequest::builder()
    .form_id("intake-1".to_string())
    .body(form.clone())
    .build()?;
  let echoed = answered!(pets.submit_intake_form(intake).await?, ApiResponse::Ok(echoed) => echoed);
  anyhow::ensure!(echoed == form, "{echoed:?}");
  let media = client::MediumRequestBody {
    file: Some(vec![vec![0; 3], vec![0; 5]]),
    ..Default::default()
  };
  let upload = client::UploadMediaRequest::builder().body(media).build()?;
  let receipt = answered!(pets.upload_media(upload).await?, ApiResponse::Ok(receipt) => receipt);
  anyhow::ensure!(receipt.sizes == [3, 5], "{receipt:?}");
  let labels = client::MediaLabels {
    additional_properties: IndexMap::from([("mood".to_string(), "sleepy".to_string())]),
  };
  let submit = client::SubmitMediaLabelsRequest::builder()
    .body(labels.clone())
    .build()?;
  let echoed = answered!(pets.submit_media_labels(submit).await?, ApiResponse::Ok(echoed) => echoed);
  anyhow::ensure!(echoed == labels, "{echoed:?}");
  let tallies = client::Tallies {
    additional_properties: IndexMap::from([("treats".to_string(), vec![1, 2])]),
  };
  let submit = client::SubmitTalliesRequest::builder().body(tallies.clone()).build()?;
  let echoed = answered!(pets.submit_tallies(submit).await?, ApiResponse::Ok(echoed) => echoed);
  anyhow::ensure!(echoed == tallies, "{echoed:?}");

  let text = answered!(pets.get_media_text(client::GetMediaTextRequest {}).await?, ApiResponse::Ok(text) => text);
  anyhow::ensure!(text == b"hi", "{text:?}");
  let wildcard = answered!(
    pets.get_media_wildcard(client::GetMediaWildcardRequest {}).await?,
    ApiResponse::Ok(bytes) => bytes
  );
  anyhow::ensure!(wildcard == [0x89, b'P', b'N', b'G'], "{wildcard:?}");
  let described = answered!(
    pets.get_media_pet_text(client::GetMediaPetTextRequest {}).await?,
    ApiResponse::Ok(client::PetOrVecu8::Pet(pet)) => pet
  );
  anyhow::ensure!(described.name == "Biscuit", "{described:?}");
  let species =
    answered!(pets.get_media_kind(client::GetMediaKindRequest {}).await?, ApiResponse::Ok(species) => species);
  anyhow::ensure!(species == client::Species::Cat, "{species:?}");
  let nickname = answered!(
    pets.get_media_nickname(client::GetMediaNicknameRequest {}).await?,
    ApiResponse::Ok(nickname) => nickname
  );
  anyhow::ensure!(nickname == "Biscuit", "{nickname:?}");
  let nullable = answered!(
    pets.get_media_nullable_name(client::GetMediaNullableNameRequest {}).await?,
    ApiResponse::Ok(name) => name
  );
  anyhow::ensure!(nullable.as_deref() == Some("Biscuit"), "{nullable:?}");
  let latin = answered!(pets.get_media_latin(client::GetMediaLatinRequest {}).await?, ApiResponse::Ok(latin) => latin);
  anyhow::ensure!(latin == "caf\u{e9}", "{latin:?}");
  let raw = client::PutMediaRawRequest::builder().body(vec![0xE9, 0x41]).build()?;
  answered!(pets.put_media_raw(raw).await?, ApiResponse::NoContent(()) => ());
  let union = answered!(
    pets.post_media_union(client::PostMediaUnionRequest {}).await?,
    ApiResponse::Ok(Some(client::CatOrPet::Pet(pet))) => pet
  );
  anyhow::ensure!(union.name == "Biscuit", "{union:?}");
  let accepted = answered!(
    pets.get_media_accepted(client::GetMediaAcceptedRequest {}).await?,
    ApiResponse::Accepted(pet) => pet
  );
  anyhow::ensure!(accepted.name == "Biscuit", "{accepted:?}");
  let both = answered!(
    pets.get_media_both(client::GetMediaBothRequest {}).await?,
    ApiResponse::Ok(client::PetOrPetStream::Pet(pet)) => pet
  );
  anyhow::ensure!(both.name == "Biscuit", "{both:?}");
  Ok(())
}

async fn staff_operations(staff: &PetstoreClient) -> anyhow::Result<()> {
  let new_key = client::NewApiKey {
    name: "kiosk".to_string(),
    value: "secret".to_string(),
  };
  let create = client::CreateApiKeyRequest::builder().body(new_key.clone()).build()?;
  let key = answered!(staff.create_api_key(create).await?, ApiResponse::Created(key) => key);
  let keys = answered!(
    staff.list_api_keys(client::ListApiKeysRequest::builder().build()?).await?,
    ApiResponse::Ok(keys) => keys
  );
  anyhow::ensure!(keys == [key.clone()], "{keys:?}");
  let get = client::GetApiKeyRequest::builder().key_id(key.id.clone()).build()?;
  let fetched = answered!(staff.get_api_key(get).await?, ApiResponse::Ok(fetched) => fetched);
  anyhow::ensure!(fetched == key, "{fetched:?}");
  let replace = client::ReplaceApiKeyRequest::builder()
    .key_id(key.id.clone())
    .body(new_key)
    .build()?;
  let replaced = answered!(staff.replace_api_key(replace).await?, ApiResponse::Ok(replaced) => replaced);
  anyhow::ensure!(replaced.id == key.id, "{replaced:?}");
  let audit = answered!(
    staff.list_audit_events(client::ListAuditEventsRequest {}).await?,
    ApiResponse::Ok(audit) => audit
  );
  anyhow::ensure!(
    audit.first().map(String::as_str) == Some("authenticated by bearer"),
    "{audit:?}"
  );
  let revoke = client::RevokeApiKeyRequest::builder().key_id(key.id).build()?;
  answered!(staff.revoke_api_key(revoke).await?, ApiResponse::NoContent(()) => ());
  Ok(())
}

async fn webhooks(petstore: &Petstore, adoption_id: i64) -> anyhow::Result<()> {
  let raw = reqwest::Client::new();
  let adopted = raw
    .post(petstore.url("/webhooks/petAdopted"))
    .json(&serde_json::json!({ "adoptionId": adoption_id, "status": "completed", "occurredAt": now() }))
    .send()
    .await?;
  anyhow::ensure!(adopted.status() == reqwest::StatusCode::OK, "{adopted:?}");
  let low = raw
    .post(petstore.url("/webhooks/inventoryLow"))
    .json(&serde_json::json!({ "species": "dog", "remaining": 1 }))
    .send()
    .await?;
  anyhow::ensure!(low.status() == reqwest::StatusCode::NO_CONTENT, "{low:?}");
  Ok(())
}

#[rstest]
#[tokio::test]
async fn every_operation_answers_with_its_declared_success(#[future(awt)] petstore: Petstore) -> anyhow::Result<()> {
  let pet = pets_operations(&petstore).await?;
  let adoption_id = store_operations(&petstore, pet.id).await?;
  assistant_operations(&petstore.client).await?;
  media_operations(&petstore.client).await?;
  staff_operations(&staff_client(&petstore)?).await?;
  let delete = client::DeletePetRequest::builder().pet_id(pet.id).build()?;
  answered!(petstore.client.delete_pet(delete).await?, ApiResponse::NoContent(()) => ());
  webhooks(&petstore, adoption_id).await?;

  let reached = petstore
    .service
    .received()
    .iter()
    .filter_map(|received| format!("{received:?}").split('(').next().map(str::to_string))
    .collect::<BTreeSet<_>>();
  let declared = OPERATIONS.into_iter().map(str::to_string).collect::<BTreeSet<_>>();
  assert_eq!(reached, declared, "every declared operation reaches the service");
  Ok(())
}
