//! Every operation in the petstore document, end to end.
//!
//! One store, one story: each generated client method is called once against the
//! generated server and answers with the success response the document declares.
//! The two webhooks, which only the server generates, are posted directly. The
//! service's request log then proves every operation reached it, so a newly added
//! operation that this file doesn't exercise fails the coverage check.

mod support;

use std::collections::BTreeSet;

use chrono::NaiveDate;
use futures::StreamExt as _;
use indexmap::IndexMap;
use petstore_client::{self as client, ApiResponse, PetstoreClient, WithHeaders};
use rstest::rstest;
use support::{API_KEY, API_VERSION, COMPATIBILITY_DATE, Petstore, QUERY_KEY, SESSION, STAFF_TOKEN, now, petstore};

const OPERATIONS: [&str; 52] = [
  "AdoptPet",
  "AdoptPetStatusChanged",
  "CreateApiKey",
  "CreatePet",
  "DeleteOrder",
  "DeletePet",
  "DescribePets",
  "GetAdoption",
  "GetApiKey",
  "GetChatPreference",
  "GetHealth",
  "GetInventory",
  "GetMediaBoth",
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
  "UpdateAssistantSettings",
  "UpdateChatPreference",
  "UnsubscribeNewsletter",
  "UpdatePet",
  "UploadMedia",
  "UploadPetImage",
];

fn staff_client(petstore: &Petstore) -> PetstoreClient {
  PetstoreClient::with_base_url(&petstore.base_url)
    .expect("the base URL parses")
    .with_api_key_auth(API_KEY)
    .with_bearer_auth(STAFF_TOKEN)
    .with_query_key(QUERY_KEY)
    .with_session_cookie(SESSION)
}

#[rstest]
#[tokio::test]
async fn every_operation_answers_with_its_declared_success(#[future(awt)] petstore: Petstore) -> anyhow::Result<()> {
  let pets = &petstore.client;
  let staff = staff_client(&petstore);

  let ApiResponse::NoContent(()) = petstore.anonymous().get_health(client::GetHealthRequest {}).await? else {
    panic!("getHealth answers 204");
  };
  let ApiResponse::Ok(trace) = petstore.anonymous().trace_health(client::TraceHealthRequest {}).await? else {
    panic!("traceHealth answers 200");
  };
  assert_eq!(trace, "TRACE /health");
  let ApiResponse::NoContent(WithHeaders { headers, .. }) = petstore
    .anonymous()
    .describe_pets(client::DescribePetsRequest {})
    .await?
  else {
    panic!("describePets answers 204");
  };
  assert_eq!(headers.allow, "GET, POST, OPTIONS");

  let new_pet = client::NewPet {
    name: "Biscuit".to_string(),
    species: client::Species::Dog,
    tags: Some(["good-boy".to_string()].into_iter().collect()),
    ..Default::default()
  };
  let ApiResponse::Created(WithHeaders { headers, body: pet }) = pets
    .create_pet(client::CreatePetRequest::builder().body(new_pet).build()?)
    .await?
  else {
    panic!("createPet answers 201");
  };
  assert_eq!(headers.location, format!("/pets/{}", pet.id));
  let pet_id = pet.id;

  let ApiResponse::Ok(WithHeaders { body: listed, .. }) = pets
    .list_pets(
      client::ListPetsRequest::builder()
        .x_compatibility_date(COMPATIBILITY_DATE)
        .build()?,
    )
    .await?
  else {
    panic!("listPets answers 200");
  };
  assert_eq!(listed.len(), 1);

  let ApiResponse::Ok(results) = pets
    .search_pets(client::SearchPetsRequest::builder().q("bisc".to_string()).build()?)
    .await?
  else {
    panic!("searchPets answers 200");
  };
  assert_eq!(results.total, 1);

  let ApiResponse::Ok(WithHeaders { body: shown, .. }) = pets
    .show_pet_by_id(
      client::ShowPetByIdRequest::builder()
        .pet_id(pet_id)
        .x_api_version(API_VERSION.to_string())
        .build()?,
    )
    .await?
  else {
    panic!("showPetById answers 200");
  };
  assert_eq!(shown, pet);

  let ApiResponse::Ok(WithHeaders { .. }) = pets
    .pet_exists(client::PetExistsRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("petExists answers 200");
  };

  let ApiResponse::Ok(updated) = pets
    .update_pet(
      client::UpdatePetRequest::builder()
        .pet_id(pet_id)
        .body(client::PetPatch {
          nickname: Some("Biscuit Jr.".to_string()),
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("updatePet answers 200");
  };
  assert_eq!(updated.nickname.as_deref(), Some("Biscuit Jr."));

  let ApiResponse::Ok(renamed) = pets
    .upload_pet_image(
      client::UploadPetImageRequest::builder()
        .pet_id(pet_id)
        .body(client::ImageRequestBody {
          name: "Sir Biscuit".to_string(),
          image: vec![0x89, b'P', b'N', b'G'],
        })
        .build()?,
    )
    .await?
  else {
    panic!("uploadPetImage answers 200");
  };
  assert_eq!(renamed.name, "Sir Biscuit");

  let ApiResponse::NoContent(()) = pets
    .replace_pet_photo(
      client::ReplacePetPhotoRequest::builder()
        .pet_id(pet_id)
        .body(vec![1, 2, 3])
        .build()?,
    )
    .await?
  else {
    panic!("replacePetPhoto answers 204");
  };
  let ApiResponse::Ok(photo) = pets
    .get_pet_photo(client::GetPetPhotoRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("getPetPhoto answers 200");
  };
  assert_eq!(photo, vec![1, 2, 3]);

  let ApiResponse::NoContent(()) = pets
    .set_pet_notes(
      client::SetPetNotesRequest::builder()
        .pet_id(pet_id)
        .body("Naps after lunch.".to_string())
        .build()?,
    )
    .await?
  else {
    panic!("setPetNotes answers 204");
  };
  let ApiResponse::Ok(card) = pets
    .get_pet_card(client::GetPetCardRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("getPetCard answers 200");
  };
  assert!(card.ends_with("Notes: Naps after lunch."), "{card}");

  let ApiResponse::Ok(lineage) = pets
    .get_pet_lineage(
      client::GetPetLineageRequest::builder()
        .pet_id(pet_id)
        .depth(1)
        .build()?,
    )
    .await?
  else {
    panic!("getPetLineage answers 200");
  };
  assert_eq!(lineage.parents.map(|parents| parents.len()), Some(2));

  let vaccine = client::VaccineRecord {
    vaccine: "rabies".to_string(),
    date_administered: NaiveDate::from_ymd_opt(2026, 3, 1).expect("a valid date"),
    ..Default::default()
  };
  let ApiResponse::Created(recorded) = pets
    .record_vaccination(
      client::RecordVaccinationRequest::builder()
        .pet_id(pet_id)
        .body(vaccine.clone())
        .build()?,
    )
    .await?
  else {
    panic!("recordVaccination answers 201");
  };
  assert_eq!(recorded, vaccine);
  let ApiResponse::Ok(vaccinations) = pets
    .list_vaccinations(client::ListVaccinationsRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("listVaccinations answers 200");
  };
  assert_eq!(vaccinations, vec![vaccine]);

  petstore.service.with_state(|state| {
    state.vitals = vec![petstore_server::VitalsReading {
      id: "reading-1".to_string(),
      heart_rate_bpm: 82,
      timestamp: Some(now()),
      ..Default::default()
    }];
  });
  let ApiResponse::Ok(vitals) = pets
    .stream_pet_vitals(client::StreamPetVitalsRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("streamPetVitals answers 200");
  };
  let readings = vitals.collect::<Vec<_>>().await;
  assert_eq!(readings.len(), 1);

  #[expect(deprecated)]
  let cats = pets.list_cats(client::ListCatsRequest::builder().build()?).await?;
  let ApiResponse::Ok(WithHeaders { body: cats, .. }) = cats else {
    panic!("listCats answers 200");
  };
  assert!(cats.is_empty(), "no pet is a cat: {cats:?}");

  let ApiResponse::Ok(inventory) = pets.get_inventory(client::GetInventoryRequest {}).await? else {
    panic!("getInventory answers 200");
  };
  assert_eq!(inventory.additional_properties.get("dog"), Some(&1));

  let ApiResponse::Accepted(WithHeaders {
    headers,
    body: adoption,
  }) = pets
    .adopt_pet(
      client::AdoptPetRequest::builder()
        .body(client::AdoptionRequest {
          pet_id,
          adopter: client::Adopter {
            name: "Avery".to_string(),
            email: "avery@example.com".to_string(),
            ..Default::default()
          },
          callback_url: Some(petstore.url("/callbacks/adoptPet/statusChanged")),
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("adoptPet answers 202");
  };
  assert_eq!(headers.location, format!("/adoptions/{}", adoption.id));
  let ApiResponse::Ok(fetched) = pets
    .get_adoption(client::GetAdoptionRequest::builder().adoption_id(adoption.id).build()?)
    .await?
  else {
    panic!("getAdoption answers 200");
  };
  assert_eq!(fetched, adoption);
  let ApiResponse::Ok(feed) = pets
    .stream_adoption_feed(client::StreamAdoptionFeedRequest::builder().build()?)
    .await?
  else {
    panic!("streamAdoptionFeed answers 200");
  };
  assert_eq!(feed.collect::<Vec<_>>().await.len(), 1);

  let ApiResponse::Created(receipt) = pets
    .submit_collar_telemetry(
      client::SubmitCollarTelemetryRequest::builder()
        .collar_id("COL-0A1B2C3D".to_string())
        .body(client::CollarTelemetry {
          collar_id: "COL-0A1B2C3D".to_string(),
          recorded_at: now(),
          location: vec![45.52, -122.68],
          samples: vec![72, 75, 80],
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("submitCollarTelemetry answers 201");
  };
  assert_eq!(receipt.reading_count, 3);

  let ApiResponse::Ok(reply) = pets
    .send_assistant_message(
      client::SendAssistantMessageRequest::builder()
        .body(client::ContentRequest {
          blocks: vec![client::ContentBlock::text("Is Biscuit due for shots?".to_string())],
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("sendAssistantMessage answers 200");
  };
  assert!(matches!(*reply.content, client::ContentBlock::Text(_)));
  let ApiResponse::Ok(client::ContentResponseOrAssistantEventFrameStream::ContentResponse(document)) = pets
    .stream_assistant_message(
      client::StreamAssistantMessageRequest::builder()
        .body(client::ContentRequest {
          blocks: vec![client::ContentBlock::text("Summarize Biscuit's visit".to_string())],
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("streamAssistantMessage answers 200 with one document when not asked to stream");
  };
  assert!(matches!(*document.content, client::ContentBlock::Text(_)));
  let ApiResponse::Ok(tokens) = pets
    .stream_assistant_tokens(client::StreamAssistantTokensRequest {})
    .await?
  else {
    panic!("streamAssistantTokens answers 200");
  };
  assert_eq!(tokens.collect::<Vec<_>>().await.len(), 0);
  let ApiResponse::Ok(events) = pets
    .list_assistant_events(
      client::ListAssistantEventsRequest::builder()
        .conversation("conversation-1".to_string())
        .build()?,
    )
    .await?
  else {
    panic!("listAssistantEvents answers 200");
  };
  assert!(!events.events.is_empty(), "the assistant replays its events");
  let ApiResponse::Ok(preference) = pets.get_chat_preference(client::GetChatPreferenceRequest {}).await? else {
    panic!("getChatPreference answers 200");
  };
  let ApiResponse::Ok(stored) = pets
    .update_chat_preference(
      client::UpdateChatPreferenceRequest::builder()
        .body(preference.clone())
        .build()?,
    )
    .await?
  else {
    panic!("updateChatPreference answers 200");
  };
  assert_eq!(stored, preference);
  let settings = client::AssistantSettings {
    sample_rate: Some(client::SampleRate::Value48000),
    ..Default::default()
  };
  let ApiResponse::Ok(echoed) = pets
    .update_assistant_settings(
      client::UpdateAssistantSettingsRequest::builder()
        .body(settings.clone())
        .build()?,
    )
    .await?
  else {
    panic!("updateAssistantSettings answers 200");
  };
  assert_eq!(echoed, settings);

  let ApiResponse::Ok(order) = pets
    .place_order(
      client::PlaceOrderRequest::builder()
        .body(client::Order {
          pet_id,
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("placeOrder answers 200");
  };
  let order_id = order.id.expect("the server assigns an id");
  let ApiResponse::Ok(fetched) = pets
    .get_order_by_id(client::GetOrderByIdRequest::builder().order_id(order_id).build()?)
    .await?
  else {
    panic!("getOrderById answers 200");
  };
  assert_eq!(fetched, order);
  let ApiResponse::NoContent(()) = pets
    .delete_order(client::DeleteOrderRequest::builder().order_id(order_id).build()?)
    .await?
  else {
    panic!("deleteOrder answers 204");
  };
  let ApiResponse::Ok(carriers) = pets.list_carriers(client::ListCarriersRequest {}).await? else {
    panic!("listCarriers answers 200");
  };
  assert_eq!(carriers.len(), 3);
  let ApiResponse::Created(subscription) = petstore
    .anonymous()
    .subscribe_newsletter(
      client::SubscribeNewsletterRequest::builder()
        .body(client::NewsletterSignup {
          email: "avery@example.com".to_string(),
          frequency: client::Frequency::Monthly,
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("subscribeNewsletter answers 201");
  };
  assert_eq!(subscription.frequency, client::Frequency::Monthly);
  let ApiResponse::NoContent(()) = petstore
    .anonymous()
    .unsubscribe_newsletter(client::UnsubscribeNewsletterRequest::builder().build()?)
    .await?
  else {
    panic!("unsubscribeNewsletter answers 204");
  };

  let form = client::IntakeForm {
    name: "Biscuit".to_string(),
    attachments: vec![b"vet-record".to_vec()],
    thumbnail: b"thumb".to_vec(),
    encoded: b"chip".to_vec(),
    count: 1,
    ..Default::default()
  };
  let ApiResponse::Ok(received) = pets
    .submit_intake_form(
      client::SubmitIntakeFormRequest::builder()
        .form_id("intake-1".to_string())
        .body(form.clone())
        .build()?,
    )
    .await?
  else {
    panic!("submitIntakeForm answers 200");
  };
  assert_eq!(received, form);
  let ApiResponse::Ok(media) = pets
    .upload_media(
      client::UploadMediaRequest::builder()
        .body(client::MediumRequestBody {
          file: Some(vec![vec![0; 3], vec![0; 5]]),
          ..Default::default()
        })
        .build()?,
    )
    .await?
  else {
    panic!("uploadMedia answers 200");
  };
  assert_eq!(media.sizes, vec![3, 5]);
  let labels = client::MediaLabels {
    additional_properties: IndexMap::from([("mood".to_string(), "sleepy".to_string())]),
  };
  let ApiResponse::Ok(echoed) = pets
    .submit_media_labels(
      client::SubmitMediaLabelsRequest::builder()
        .body(labels.clone())
        .build()?,
    )
    .await?
  else {
    panic!("submitMediaLabels answers 200");
  };
  assert_eq!(echoed, labels);
  let tallies = client::Tallies {
    additional_properties: IndexMap::from([("treats".to_string(), vec![1, 2])]),
  };
  let ApiResponse::Ok(echoed) = pets
    .submit_tallies(client::SubmitTalliesRequest::builder().body(tallies.clone()).build()?)
    .await?
  else {
    panic!("submitTallies answers 200");
  };
  assert_eq!(echoed, tallies);

  let ApiResponse::Ok(client::PetOrPetStream::Pet(streamed)) =
    pets.get_media_both(client::GetMediaBothRequest {}).await?
  else {
    panic!("getMediaBoth answers 200 with a pet");
  };
  assert_eq!(streamed.name, "Biscuit");

  let new_key = client::NewApiKey {
    name: "kiosk".to_string(),
    value: "secret".to_string(),
  };
  let ApiResponse::Created(key) = staff
    .create_api_key(client::CreateApiKeyRequest::builder().body(new_key.clone()).build()?)
    .await?
  else {
    panic!("createApiKey answers 201");
  };
  let ApiResponse::Ok(keys) = staff
    .list_api_keys(client::ListApiKeysRequest::builder().build()?)
    .await?
  else {
    panic!("listApiKeys answers 200");
  };
  assert_eq!(keys, vec![key.clone()]);
  let ApiResponse::Ok(fetched) = staff
    .get_api_key(client::GetApiKeyRequest::builder().key_id(key.id.clone()).build()?)
    .await?
  else {
    panic!("getApiKey answers 200");
  };
  assert_eq!(fetched, key);
  let ApiResponse::Ok(replaced) = staff
    .replace_api_key(
      client::ReplaceApiKeyRequest::builder()
        .key_id(key.id.clone())
        .body(new_key)
        .build()?,
    )
    .await?
  else {
    panic!("replaceApiKey answers 200");
  };
  assert_eq!(replaced.id, key.id);
  let ApiResponse::Ok(audit) = staff.list_audit_events(client::ListAuditEventsRequest {}).await? else {
    panic!("listAuditEvents answers 200");
  };
  assert_eq!(audit.first().map(String::as_str), Some("authenticated by bearer"));
  let ApiResponse::NoContent(()) = staff
    .revoke_api_key(client::RevokeApiKeyRequest::builder().key_id(key.id).build()?)
    .await?
  else {
    panic!("revokeApiKey answers 204");
  };

  let ApiResponse::NoContent(()) = pets
    .delete_pet(client::DeletePetRequest::builder().pet_id(pet_id).build()?)
    .await?
  else {
    panic!("deletePet answers 204");
  };

  let raw = reqwest::Client::new();
  let adopted = raw
    .post(petstore.url("/webhooks/petAdopted"))
    .json(&serde_json::json!({ "adoptionId": adoption.id, "status": "completed", "occurredAt": now() }))
    .send()
    .await?;
  assert_eq!(adopted.status(), reqwest::StatusCode::OK);
  let low = raw
    .post(petstore.url("/webhooks/inventoryLow"))
    .json(&serde_json::json!({ "species": "dog", "remaining": 1 }))
    .send()
    .await?;
  assert_eq!(low.status(), reqwest::StatusCode::NO_CONTENT);

  let reached = petstore
    .service
    .received()
    .iter()
    .map(|received| {
      format!("{received:?}")
        .split('(')
        .next()
        .unwrap_or_default()
        .to_string()
    })
    .collect::<BTreeSet<_>>();
  let declared = OPERATIONS.into_iter().map(str::to_string).collect::<BTreeSet<_>>();
  assert_eq!(reached, declared, "every declared operation reaches the service");
  Ok(())
}
