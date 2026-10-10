//! An in-memory pet store behind the generated `ApiServer` trait.
//!
//! The service behaves like a small real deployment: it stores what clients send,
//! enforces the credential values the security schemes name, paginates, filters,
//! and answers with every status code the document declares. Each request is also
//! recorded as [`Received`], so tests can assert exactly what the generated server
//! extracted from the wire.

use std::{
  collections::{HashMap, HashSet},
  sync::{Arc, Mutex},
};

use axum::http::StatusCode;
use chrono::Datelike as _;
use indexmap::{IndexMap, IndexSet};
use oas3_gen_support::ServerEvents;
use petstore_server as server;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;
use validator::Validate;

use super::{API_KEY, API_VERSION, AUDIT_TOKEN, COMPATIBILITY_DATE, QUERY_KEY, SESSION, STAFF_TOKEN, now};

type Response<Value, Failure = ()> = anyhow::Result<server::ApiResponse<Value, Failure>>;

/// One request as the generated server handed it to the service.
#[derive(Debug, Clone)]
pub enum Received {
  ListPets(server::ListPetsRequest),
  CreatePet(server::CreatePetRequest),
  DescribePets(server::DescribePetsRequest),
  SearchPets(server::SearchPetsRequest),
  ShowPetById(server::ShowPetByIdRequest),
  DeletePet(server::DeletePetRequest),
  PetExists(server::PetExistsRequest),
  UpdatePet(server::UpdatePetRequest),
  UploadPetImage(server::UploadPetImageRequest),
  GetPetPhoto(server::GetPetPhotoRequest),
  ReplacePetPhoto(server::ReplacePetPhotoRequest),
  GetPetCard(server::GetPetCardRequest),
  SetPetNotes(server::SetPetNotesRequest),
  GetPetLineage(server::GetPetLineageRequest),
  ListVaccinations(server::ListVaccinationsRequest),
  RecordVaccination(server::RecordVaccinationRequest),
  StreamPetVitals(server::StreamPetVitalsRequest),
  ListCats(server::ListCatsRequest),
  AdoptPet(server::AdoptPetRequest),
  StreamAdoptionFeed(server::StreamAdoptionFeedRequest),
  GetAdoption(server::GetAdoptionRequest),
  SubmitCollarTelemetry(server::SubmitCollarTelemetryRequest),
  SendAssistantMessage(server::SendAssistantMessageRequest),
  ListAssistantEvents(server::ListAssistantEventsRequest),
  GetChatPreference(server::GetChatPreferenceRequest),
  UpdateChatPreference(server::UpdateChatPreferenceRequest),
  UpdateAssistantSettings(server::UpdateAssistantSettingsRequest),
  GetInventory(server::GetInventoryRequest),
  PlaceOrder(server::PlaceOrderRequest),
  GetOrderById(server::GetOrderByIdRequest),
  DeleteOrder(server::DeleteOrderRequest),
  ListCarriers(server::ListCarriersRequest),
  SubscribeNewsletter(server::SubscribeNewsletterRequest),
  UnsubscribeNewsletter(server::UnsubscribeNewsletterRequest),
  SubmitIntakeForm(server::SubmitIntakeFormRequest),
  UploadMedia(server::UploadMediaRequest),
  SubmitMediaLabels(server::SubmitMediaLabelsRequest),
  SubmitTallies(server::SubmitTalliesRequest),
  ListApiKeys(server::ListApiKeysRequest),
  CreateApiKey(server::CreateApiKeyRequest),
  GetApiKey(server::GetApiKeyRequest),
  ReplaceApiKey(server::ReplaceApiKeyRequest),
  RevokeApiKey(server::RevokeApiKeyRequest),
  ListAuditEvents(server::ListAuditEventsRequest),
  GetHealth(server::GetHealthRequest),
  TraceHealth(server::TraceHealthRequest),
  AdoptPetStatusChanged(server::AdoptPetStatusChangedRequest),
  PetAdopted(server::PetAdoptedRequest),
  InventoryLow(server::InventoryLowRequest),
}

/// Everything the store holds. Tests seed and inspect it through
/// [`PetstoreService::with_state`].
#[derive(Debug, Default)]
pub struct State {
  pub pets: IndexMap<Uuid, server::Pet>,
  pub revisions: HashMap<Uuid, u64>,
  pub cached: HashSet<Uuid>,
  pub notes: HashMap<Uuid, String>,
  pub photos: HashMap<Uuid, Vec<u8>>,
  pub vaccinations: HashMap<Uuid, Vec<server::VaccineRecord>>,
  pub vitals: Vec<server::VitalsReading>,
  pub feed: Vec<server::AdoptionFeedEvent>,
  pub adoptions: IndexMap<i64, server::Adoption>,
  pub orders: IndexMap<i64, server::Order>,
  pub carriers: Vec<server::PetCarrier>,
  pub api_keys: IndexMap<String, server::ApiKey>,
  pub preference: Option<server::ChatPreference>,
  /// The status each `statusChanged` callback delivery got back, in delivery order.
  pub callback_deliveries: Vec<u16>,
  pub received: Vec<Received>,
  next_id: u64,
}

impl State {
  fn next_id(&mut self) -> u64 {
    self.next_id += 1;
    self.next_id
  }

  fn etag(&self, id: Uuid) -> String {
    format!("\"{id}-{}\"", self.revisions.get(&id).copied().unwrap_or_default())
  }

  fn touch(&mut self, id: Uuid) {
    *self.revisions.entry(id).or_default() += 1;
  }
}

/// The in-memory store. Clones share one [`State`].
#[derive(Debug, Clone, Default)]
pub struct PetstoreService {
  state: Arc<Mutex<State>>,
}

impl PetstoreService {
  /// Runs `f` with exclusive access to the store's state.
  pub fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut self.state.lock().expect("the state lock is never poisoned"))
  }

  /// Every request received so far, oldest first.
  pub fn received(&self) -> Vec<Received> {
    self.with_state(|state| state.received.clone())
  }

  /// The most recent request.
  pub fn last_received(&self) -> Option<Received> {
    self.with_state(|state| state.received.last().cloned())
  }

  /// Stores `pet` as is and returns its id.
  pub fn insert_pet(&self, pet: server::Pet) -> Uuid {
    self.with_state(|state| {
      let id = pet.id;
      state.pets.insert(id, pet);
      state.touch(id);
      id
    })
  }

  fn record(&self, received: Received) {
    self.with_state(|state| state.received.push(received));
  }
}

fn error(status: StatusCode, message: impl Into<String>) -> server::Error {
  server::Error {
    code: i32::from(status.as_u16()),
    message: message.into(),
  }
}

fn problem(status: StatusCode, detail: impl Into<String>, instance: impl Into<String>) -> server::Problem {
  server::Problem {
    r#type: Some("about:blank".to_string()),
    title: status.canonical_reason().map(str::to_string),
    status: Some(i32::from(status.as_u16())),
    detail: Some(detail.into()),
    instance: Some(instance.into()),
    ..Default::default()
  }
}

fn pet_not_found(id: Uuid) -> server::Problem {
  problem(
    StatusCode::NOT_FOUND,
    format!("no pet has id {id}"),
    format!("/pets/{id}"),
  )
}

fn exposes(secret: Option<&SecretString>, expected: &str) -> bool {
  secret.is_some_and(|secret| secret.expose_secret() == expected)
}

fn wire_name<T: serde::Serialize>(value: &T) -> String {
  serde_json::to_value(value)
    .ok()
    .and_then(|value| value.as_str().map(str::to_string))
    .unwrap_or_default()
}

fn sort_by_name<T>(
  items: &mut [T],
  order: Option<&server::ListPetsRequestHeaderXSortOrder>,
  name: impl Fn(&T) -> &str,
) {
  match order {
    Some(server::ListPetsRequestHeaderXSortOrder::Asc) => items.sort_by(|a, b| name(a).cmp(name(b))),
    Some(server::ListPetsRequestHeaderXSortOrder::Desc) => items.sort_by(|a, b| name(b).cmp(name(a))),
    None => {}
  }
}

/// A deterministic family tree `depth` generations deep above `name`.
fn lineage(id: Uuid, name: &str, species: &server::Species, depth: i32) -> server::Lineage {
  let parents = (depth > 0).then(|| {
    ["sire", "dam"]
      .into_iter()
      .enumerate()
      .map(|(index, role)| {
        let parent = Uuid::from_u128(id.as_u128().wrapping_mul(3).wrapping_add(index as u128 + 1));
        lineage(parent, &format!("{name}'s {role}"), species, depth - 1)
      })
      .collect()
  });
  server::Lineage {
    pet_id: id,
    name: name.to_string(),
    species: Some(species.clone()),
    parents,
  }
}

fn cat_from(pet: &server::Pet) -> server::Cat {
  server::Cat {
    id: pet.id,
    name: pet.name.clone(),
    species: pet.species.clone(),
    status: pet.status.clone(),
    nickname: pet.nickname.clone(),
    birth_date: pet.birth_date,
    weight_kg: pet.weight_kg,
    microchip: pet.microchip.clone(),
    tags: pet.tags.clone(),
    photo_urls: pet.photo_urls.clone(),
    created_at: pet.created_at,
    lives: 9,
    ..Default::default()
  }
}

fn assistant_events() -> server::AssistantEventList {
  let text = server::ContentBlock::Text(server::TextBlock {
    text: "Biscuit is due for a booster.".to_string(),
    ..Default::default()
  });
  server::AssistantEventList {
    events: vec![
      server::AssistantEvent::MessageStart(server::MessageStartEvent {
        message: server::Message {
          id: "msg_1".to_string(),
          role: server::Role::Assistant,
          content: vec![],
          model: "chat-1.1".to_string(),
          stop_reason: None,
          usage: Some(server::Usage {
            input_tokens: 12,
            output_tokens: 0,
          }),
        },
        ..Default::default()
      }),
      server::AssistantEvent::ContentBlockStart(server::ContentBlockStartEvent {
        index: 0,
        content_block: Box::new(text),
        ..Default::default()
      }),
      server::AssistantEvent::ContentBlockDelta(server::ContentBlockDeltaEvent {
        index: 0,
        delta: Box::new(server::Delta::Text(server::TextDelta {
          text: " Book it this week.".to_string(),
          ..Default::default()
        })),
        ..Default::default()
      }),
      server::AssistantEvent::ContentBlockDelta(server::ContentBlockDeltaEvent {
        index: 0,
        delta: Box::new(server::Delta::InputJson(server::InputJsonDelta {
          partial_json: "{\"vaccine\":".to_string(),
          ..Default::default()
        })),
        ..Default::default()
      }),
      server::AssistantEvent::ContentBlockStop(server::ContentBlockStopEvent {
        index: 0,
        ..Default::default()
      }),
      server::AssistantEvent::Ping(server::PingEvent::default()),
      server::AssistantEvent::MessageStop(server::MessageStopEvent::default()),
    ],
  }
}

fn default_carriers() -> Vec<server::PetCarrier> {
  vec![
    server::PetCarrier {
      id: "rolling-crate".to_string(),
      wheels: Some(4),
      straps: None,
    },
    server::PetCarrier {
      id: "trail-pack".to_string(),
      wheels: None,
      straps: Some(2),
    },
    server::PetCarrier {
      id: "convertible".to_string(),
      wheels: Some(2),
      straps: Some(1),
    },
  ]
}

#[allow(
  clippy::unused_async_trait_impl,
  reason = "the generated trait returns futures, and an in-memory store never waits"
)]
impl server::ApiServer for PetstoreService {
  async fn list_pets(
    &self,
    request: server::ListPetsRequest,
  ) -> Response<server::WithHeaders<server::XNextAndXTotalCountAndXRequestIdHeaders, server::Pets>, server::Error> {
    self.record(Received::ListPets(request.clone()));
    let server::ListPetsRequest { query, header, .. } = request;

    if header.x_compatibility_date != COMPATIBILITY_DATE {
      return Ok(server::ApiResponse::Unknown(
        StatusCode::BAD_REQUEST,
        error(StatusCode::BAD_REQUEST, "unsupported compatibility date"),
      ));
    }
    let Ok(offset) = query.cursor.as_deref().unwrap_or("0").parse::<usize>() else {
      return Ok(server::ApiResponse::Unknown(
        StatusCode::BAD_REQUEST,
        error(StatusCode::BAD_REQUEST, "malformed cursor"),
      ));
    };
    let limit = usize::try_from(query.limit.unwrap_or(20)).unwrap_or_default();

    let mut matching = self.with_state(|state| {
      state
        .pets
        .values()
        .filter(|pet| {
          query
            .status
            .as_ref()
            .is_none_or(|status| pet.status.as_ref() == Some(status))
        })
        .filter(|pet| {
          query.tags.as_ref().is_none_or(|wanted| {
            wanted
              .iter()
              .all(|tag| pet.tags.as_ref().is_some_and(|tags| tags.contains(tag)))
          })
        })
        .filter(|pet| header.x_only.as_ref().is_none_or(|only| only.contains(&pet.species)))
        .cloned()
        .collect::<Vec<_>>()
    });
    sort_by_name(&mut matching, header.x_sort_order.as_ref(), |pet| &pet.name);

    let total = matching.len();
    let page = matching.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
    let next = offset + page.len();

    Ok(server::ApiResponse::Ok(server::WithHeaders {
      headers: server::XNextAndXTotalCountAndXRequestIdHeaders {
        x_next: (next < total).then(|| next.to_string()),
        x_total_count: i64::try_from(total).ok(),
        x_request_id: header.x_request_id,
      },
      body: page,
    }))
  }

  async fn create_pet(
    &self,
    request: server::CreatePetRequest,
  ) -> Response<server::WithHeaders<server::LocationHeaders, server::Pet>, server::Error> {
    self.record(Received::CreatePet(request.clone()));
    let body = request.body;
    if let Err(errors) = body.validate() {
      return Ok(server::ApiResponse::BadRequest(error(
        StatusCode::BAD_REQUEST,
        errors.to_string(),
      )));
    }

    let pet = self.with_state(|state| {
      let id = Uuid::from_u128(u128::from(state.next_id()));
      let pet = server::Pet {
        id,
        name: body.name,
        species: body.species,
        status: Some(server::PetStatus::Available),
        nickname: body.nickname,
        birth_date: body.birth_date,
        weight_kg: body.weight_kg,
        microchip: body.microchip,
        tags: body.tags,
        created_at: Some(now()),
        ..Default::default()
      };
      state.pets.insert(id, pet.clone());
      state.touch(id);
      pet
    });

    Ok(server::ApiResponse::Created(server::WithHeaders {
      headers: server::LocationHeaders {
        location: format!("/pets/{}", pet.id),
      },
      body: pet,
    }))
  }

  async fn describe_pets(
    &self,
    request: server::DescribePetsRequest,
  ) -> Response<server::WithHeaders<server::AllowHeaders, ()>> {
    self.record(Received::DescribePets(request));
    Ok(server::ApiResponse::NoContent(server::WithHeaders {
      headers: server::AllowHeaders {
        allow: "GET, POST, OPTIONS".to_string(),
      },
      body: (),
    }))
  }

  async fn search_pets(&self, request: server::SearchPetsRequest) -> Response<server::SearchResults, server::Error> {
    self.record(Received::SearchPets(request.clone()));
    let query = request.query;
    let needle = query.q.to_lowercase();

    let items = self.with_state(|state| {
      state
        .pets
        .values()
        .filter(|pet| {
          pet.name.to_lowercase().contains(&needle)
            || pet
              .nickname
              .as_ref()
              .is_some_and(|nickname| nickname.to_lowercase().contains(&needle))
            || pet
              .tags
              .as_ref()
              .is_some_and(|tags| tags.iter().any(|tag| tag.to_lowercase().contains(&needle)))
        })
        .filter(|pet| {
          query
            .species
            .as_ref()
            .is_none_or(|species| species.contains(&wire_name(&pet.species)))
        })
        .filter(|pet| {
          query.colors.as_ref().is_none_or(|colors| {
            pet
              .tags
              .as_ref()
              .is_some_and(|tags| colors.iter().any(|color| tags.contains(color)))
          })
        })
        .filter(|pet| {
          query
            .min_weight_kg
            .is_none_or(|minimum| pet.weight_kg.is_some_and(|weight| weight >= minimum))
        })
        .filter(|pet| {
          query.vaccinated.is_none_or(|vaccinated| {
            state
              .vaccinations
              .get(&pet.id)
              .is_some_and(|records| !records.is_empty())
              == vaccinated
          })
        })
        .filter(|pet| {
          query
            .born_after
            .is_none_or(|after| pet.birth_date.is_some_and(|born| born > after))
        })
        .filter(|pet| {
          query.tags.as_ref().is_none_or(|wanted| {
            pet
              .tags
              .as_ref()
              .is_some_and(|tags| wanted.iter().any(|tag| tags.contains(tag)))
          })
        })
        .filter(|pet| {
          query
            .statuses
            .as_ref()
            .is_none_or(|statuses| pet.status.as_ref().is_some_and(|status| statuses.contains(status)))
        })
        .filter(|pet| {
          query
            .born_in
            .as_ref()
            .is_none_or(|years| pet.birth_date.is_some_and(|born| years.contains(&born.year())))
        })
        .cloned()
        .collect::<Vec<_>>()
    });

    Ok(server::ApiResponse::Ok(server::SearchResults {
      total: i64::try_from(items.len()).unwrap_or_default(),
      items,
      next_cursor: None,
    }))
  }

  async fn show_pet_by_id(
    &self,
    request: server::ShowPetByIdRequest,
  ) -> Response<server::WithHeaders<server::ETagAndXCacheHeaders, server::Pet>, server::ErrorOrProblem> {
    self.record(Received::ShowPetById(request.clone()));
    let id = request.path.pet_id;

    if request.header.x_api_version != API_VERSION {
      return Ok(server::ApiResponse::ClientError(
        StatusCode::BAD_REQUEST,
        server::ErrorOrProblem::Error(error(
          StatusCode::BAD_REQUEST,
          format!("unsupported API version `{}`", request.header.x_api_version),
        )),
      ));
    }

    self.with_state(|state| {
      let Some(pet) = state.pets.get(&id).cloned() else {
        return Ok(server::ApiResponse::NotFound(server::ErrorOrProblem::Problem(
          pet_not_found(id),
        )));
      };
      let etag = state.etag(id);
      if request.header.if_none_match.as_deref() == Some(etag.as_str()) {
        return Ok(server::ApiResponse::NotModified);
      }
      let x_cache = if state.cached.insert(id) {
        server::ShowPetByIdResponseHeaderXCache::Miss
      } else {
        server::ShowPetByIdResponseHeaderXCache::Hit
      };
      Ok(server::ApiResponse::Ok(server::WithHeaders {
        headers: server::ETagAndXCacheHeaders {
          etag: Some(etag),
          x_cache: Some(x_cache),
        },
        body: pet,
      }))
    })
  }

  async fn delete_pet(&self, request: server::DeletePetRequest) -> Response<(), server::Problem> {
    self.record(Received::DeletePet(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if state.pets.shift_remove(&id).is_some() {
        state.cached.remove(&id);
        Ok(server::ApiResponse::NoContent(()))
      } else {
        Ok(server::ApiResponse::NotFound(pet_not_found(id)))
      }
    })
  }

  async fn pet_exists(
    &self,
    request: server::PetExistsRequest,
  ) -> Response<server::WithHeaders<server::ETagHeaders, ()>> {
    self.record(Received::PetExists(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if state.pets.contains_key(&id) {
        Ok(server::ApiResponse::Ok(server::WithHeaders {
          headers: server::ETagHeaders {
            etag: Some(state.etag(id)),
          },
          body: (),
        }))
      } else {
        Ok(server::ApiResponse::NotFound(()))
      }
    })
  }

  async fn update_pet(&self, request: server::UpdatePetRequest) -> Response<server::Pet, server::Problem> {
    self.record(Received::UpdatePet(request.clone()));
    let id = request.path.pet_id;
    let patch = request.body;
    if let Err(errors) = patch.validate() {
      return Ok(server::ApiResponse::UnprocessableEntity(problem(
        StatusCode::UNPROCESSABLE_ENTITY,
        errors.to_string(),
        format!("/pets/{id}"),
      )));
    }

    self.with_state(|state| {
      let Some(pet) = state.pets.get_mut(&id) else {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      };
      if let Some(name) = patch.name {
        pet.name = name;
      }
      if let Some(status) = patch.status {
        pet.status = Some(status);
      }
      if let Some(nickname) = patch.nickname {
        pet.nickname = Some(nickname);
      }
      if let Some(weight) = patch.weight_kg {
        pet.weight_kg = Some(weight);
      }
      if let Some(tags) = patch.tags {
        pet.tags = Some(tags.into_iter().collect::<IndexSet<_>>());
      }
      let pet = pet.clone();
      state.touch(id);
      Ok(server::ApiResponse::Ok(pet))
    })
  }

  async fn upload_pet_image(&self, request: server::UploadPetImageRequest) -> Response<server::Pet, server::Problem> {
    self.record(Received::UploadPetImage(request.clone()));
    let id = request.path.pet_id;
    let server::ImageRequestBody { name, image } = request.body;
    self.with_state(|state| {
      let Some(pet) = state.pets.get_mut(&id) else {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      };
      pet.name = name;
      pet.photo_urls = Some(vec![format!("/pets/{id}/photo")]);
      let pet = pet.clone();
      state.photos.insert(id, image);
      state.touch(id);
      Ok(server::ApiResponse::Ok(pet))
    })
  }

  async fn get_pet_photo(&self, request: server::GetPetPhotoRequest) -> Response<Vec<u8>, server::Problem> {
    self.record(Received::GetPetPhoto(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| match state.photos.get(&id) {
      Some(photo) => Ok(server::ApiResponse::Ok(photo.clone())),
      None => Ok(server::ApiResponse::NotFound(pet_not_found(id))),
    })
  }

  async fn replace_pet_photo(&self, request: server::ReplacePetPhotoRequest) -> Response<(), server::Problem> {
    self.record(Received::ReplacePetPhoto(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if !state.pets.contains_key(&id) {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      }
      state.photos.insert(id, request.body);
      Ok(server::ApiResponse::NoContent(()))
    })
  }

  async fn get_pet_card(&self, request: server::GetPetCardRequest) -> Response<String, server::Problem> {
    self.record(Received::GetPetCard(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      let Some(pet) = state.pets.get(&id) else {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      };
      let status = pet.status.as_ref().map(wire_name).unwrap_or_default();
      let notes = state.notes.get(&id).map(String::as_str).unwrap_or_default();
      Ok(server::ApiResponse::Ok(format!(
        "{} ({})\nStatus: {status}\nNotes: {notes}",
        pet.name,
        wire_name(&pet.species)
      )))
    })
  }

  async fn set_pet_notes(&self, request: server::SetPetNotesRequest) -> Response<(), server::Problem> {
    self.record(Received::SetPetNotes(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if !state.pets.contains_key(&id) {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      }
      state.notes.insert(id, request.body);
      Ok(server::ApiResponse::NoContent(()))
    })
  }

  async fn get_pet_lineage(&self, request: server::GetPetLineageRequest) -> Response<server::Lineage, server::Problem> {
    self.record(Received::GetPetLineage(request.clone()));
    let id = request.path.pet_id;
    let depth = request.query.depth.unwrap_or(2);
    self.with_state(|state| match state.pets.get(&id) {
      Some(pet) => Ok(server::ApiResponse::Ok(lineage(id, &pet.name, &pet.species, depth))),
      None => Ok(server::ApiResponse::NotFound(pet_not_found(id))),
    })
  }

  async fn list_vaccinations(
    &self,
    request: server::ListVaccinationsRequest,
  ) -> Response<server::Vaccination200Response, server::Problem> {
    self.record(Received::ListVaccinations(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if !state.pets.contains_key(&id) {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      }
      Ok(server::ApiResponse::Ok(
        state.vaccinations.get(&id).cloned().unwrap_or_default(),
      ))
    })
  }

  async fn record_vaccination(
    &self,
    request: server::RecordVaccinationRequest,
  ) -> Response<server::VaccineRecord, server::Problem> {
    self.record(Received::RecordVaccination(request.clone()));
    let id = request.path.pet_id;
    self.with_state(|state| {
      if !state.pets.contains_key(&id) {
        return Ok(server::ApiResponse::NotFound(pet_not_found(id)));
      }
      state.vaccinations.entry(id).or_default().push(request.body.clone());
      Ok(server::ApiResponse::Created(request.body))
    })
  }

  async fn stream_pet_vitals(
    &self,
    request: server::StreamPetVitalsRequest,
  ) -> Response<ServerEvents<server::VitalsReading>> {
    self.record(Received::StreamPetVitals(request.clone()));
    let since = request.query.since;
    let readings = self.with_state(|state| {
      state
        .vitals
        .iter()
        .filter(|reading| since.is_none_or(|since| reading.timestamp.is_some_and(|at| at > since)))
        .cloned()
        .collect::<Vec<_>>()
    });
    Ok(server::ApiResponse::Ok(readings.into_iter().collect()))
  }

  async fn list_cats(
    &self,
    request: server::ListCatsRequest,
  ) -> Response<server::WithHeaders<server::XNextHeaders, server::Cats>, server::Error> {
    self.record(Received::ListCats(request.clone()));
    let limit = usize::try_from(request.query.limit.unwrap_or(20)).unwrap_or_default();
    let mut cats = self.with_state(|state| {
      state
        .pets
        .values()
        .filter(|pet| pet.species == server::Species::Cat)
        .map(cat_from)
        .collect::<Vec<_>>()
    });
    sort_by_name(&mut cats, request.header.x_sort_order.as_ref(), |cat| &cat.name);
    cats.truncate(limit);
    Ok(server::ApiResponse::Ok(server::WithHeaders {
      headers: server::XNextHeaders { x_next: None },
      body: cats,
    }))
  }

  async fn adopt_pet(
    &self,
    request: server::AdoptPetRequest,
  ) -> Response<server::WithHeaders<server::LocationHeaders, server::Adoption>, server::Error> {
    self.record(Received::AdoptPet(request.clone()));
    let body = request.body;
    let callback_url = body.callback_url.clone();
    let response = self.with_state(|state| {
      let Some(pet) = state.pets.get_mut(&body.pet_id) else {
        return server::ApiResponse::Conflict(error(StatusCode::CONFLICT, format!("no pet has id {}", body.pet_id)));
      };
      if pet
        .status
        .as_ref()
        .is_some_and(|status| *status != server::PetStatus::Available)
      {
        return server::ApiResponse::Conflict(error(StatusCode::CONFLICT, format!("{} is not available", pet.name)));
      }
      pet.status = Some(server::PetStatus::Pending);
      let name = pet.name.clone();
      state.touch(body.pet_id);

      let id = i64::try_from(state.next_id()).unwrap_or_default();
      let adoption = server::Adoption {
        id,
        pet_id: body.pet_id,
        adopter: Some(body.adopter),
        status: server::AdoptionStatus::Submitted,
        submitted_at: now(),
      };
      state.adoptions.insert(id, adoption.clone());
      state.feed.push(server::AdoptionFeedEvent {
        r#type: server::AdoptionFeedEventType::Created,
        payload: server::AdoptionFeedPayload {
          id: id.to_string(),
          name: Some(name),
          value: Some(id),
        },
      });

      server::ApiResponse::Accepted(server::WithHeaders {
        headers: server::LocationHeaders {
          location: format!("/adoptions/{id}"),
        },
        body: adoption,
      })
    });

    if let server::ApiResponse::Accepted(accepted) = &response
      && let Some(callback_url) = callback_url
    {
      let event = serde_json::json!({
        "adoptionId": accepted.body.id,
        "petId": accepted.body.pet_id,
        "status": "submitted",
        "occurredAt": now(),
      });
      let delivered = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?
        .post(callback_url)
        .json(&event)
        .send()
        .await
        .map_or(0, |response| response.status().as_u16());
      self.with_state(|state| state.callback_deliveries.push(delivered));
    }
    Ok(response)
  }

  async fn stream_adoption_feed(
    &self,
    request: server::StreamAdoptionFeedRequest,
  ) -> Response<ServerEvents<server::AdoptionFeedEvent>> {
    self.record(Received::StreamAdoptionFeed(request.clone()));
    let filter = request.query.filter;
    let events = self.with_state(|state| {
      state
        .feed
        .iter()
        .filter(|event| {
          filter
            .as_deref()
            .is_none_or(|filter| event.payload.name.as_deref().is_some_and(|name| name.contains(filter)))
        })
        .cloned()
        .collect::<Vec<_>>()
    });
    Ok(server::ApiResponse::Ok(events.into_iter().collect()))
  }

  async fn get_adoption(&self, request: server::GetAdoptionRequest) -> Response<server::Adoption, server::Problem> {
    self.record(Received::GetAdoption(request.clone()));
    let id = request.path.adoption_id;
    self.with_state(|state| match state.adoptions.get(&id) {
      Some(adoption) => Ok(server::ApiResponse::Ok(adoption.clone())),
      None => Ok(server::ApiResponse::NotFound(problem(
        StatusCode::NOT_FOUND,
        format!("no adoption has id {id}"),
        format!("/adoptions/{id}"),
      ))),
    })
  }

  async fn submit_collar_telemetry(
    &self,
    request: server::SubmitCollarTelemetryRequest,
  ) -> Response<server::TelemetryReceipt, server::Error> {
    self.record(Received::SubmitCollarTelemetry(request.clone()));
    let telemetry = request.body;
    if let Err(errors) = telemetry.validate() {
      return Ok(server::ApiResponse::BadRequest(error(
        StatusCode::BAD_REQUEST,
        errors.to_string(),
      )));
    }
    Ok(server::ApiResponse::Created(server::TelemetryReceipt {
      accepted: true,
      reading_count: i32::try_from(telemetry.samples.len()).unwrap_or(i32::MAX),
      received_at: Some(now()),
    }))
  }

  async fn send_assistant_message(
    &self,
    request: server::SendAssistantMessageRequest,
  ) -> Response<server::ContentResponse, server::ErrorResponse> {
    self.record(Received::SendAssistantMessage(request.clone()));
    let blocks = request.body.blocks;
    let Some(last) = blocks.last().cloned() else {
      return Ok(server::ApiResponse::BadRequest(server::ErrorResponse {
        error: server::ErrorDetails {
          r#type: server::ErrorType::InvalidRequestError,
          message: "blocks must not be empty".to_string(),
        },
        ..Default::default()
      }));
    };
    let id = self.with_state(State::next_id);
    Ok(server::ApiResponse::Ok(server::ContentResponse {
      id: format!("msg_{id}"),
      content: Box::new(last),
      usage: Some(server::Usage {
        input_tokens: i64::try_from(blocks.len()).unwrap_or_default(),
        output_tokens: 1,
      }),
    }))
  }

  async fn list_assistant_events(
    &self,
    request: server::ListAssistantEventsRequest,
  ) -> Response<server::AssistantEventList> {
    self.record(Received::ListAssistantEvents(request));
    Ok(server::ApiResponse::Ok(assistant_events()))
  }

  async fn get_chat_preference(&self, request: server::GetChatPreferenceRequest) -> Response<server::ChatPreference> {
    self.record(Received::GetChatPreference(request));
    let preference = self.with_state(|state| {
      state.preference.clone().unwrap_or_else(|| server::ChatPreference {
        model: server::ChatModelId::Chat11,
        ..Default::default()
      })
    });
    Ok(server::ApiResponse::Ok(preference))
  }

  async fn update_chat_preference(
    &self,
    request: server::UpdateChatPreferenceRequest,
  ) -> Response<server::ChatPreference> {
    self.record(Received::UpdateChatPreference(request.clone()));
    let preference = request.body;
    self.with_state(|state| state.preference = Some(preference.clone()));
    Ok(server::ApiResponse::Ok(preference))
  }

  async fn update_assistant_settings(
    &self,
    request: server::UpdateAssistantSettingsRequest,
  ) -> Response<server::AssistantSettings> {
    self.record(Received::UpdateAssistantSettings(request.clone()));
    Ok(server::ApiResponse::Ok(request.body))
  }

  async fn get_inventory(&self, request: server::GetInventoryRequest) -> Response<server::Inventory> {
    self.record(Received::GetInventory(request));
    let inventory = self.with_state(|state| {
      state
        .pets
        .values()
        .filter(|pet| {
          pet
            .status
            .as_ref()
            .is_none_or(|status| *status == server::PetStatus::Available)
        })
        .fold(IndexMap::<String, i32>::new(), |mut counts, pet| {
          *counts.entry(wire_name(&pet.species)).or_default() += 1;
          counts
        })
    });
    Ok(server::ApiResponse::Ok(server::Inventory {
      additional_properties: inventory,
    }))
  }

  async fn place_order(&self, request: server::PlaceOrderRequest) -> Response<server::Order, server::Error> {
    self.record(Received::PlaceOrder(request.clone()));
    let order = request.body;
    if let Err(errors) = order.validate() {
      return Ok(server::ApiResponse::BadRequest(error(
        StatusCode::BAD_REQUEST,
        errors.to_string(),
      )));
    }
    self.with_state(|state| {
      if !state.pets.contains_key(&order.pet_id) {
        return Ok(server::ApiResponse::BadRequest(error(
          StatusCode::BAD_REQUEST,
          format!("no pet has id {}", order.pet_id),
        )));
      }
      let id = i64::try_from(state.next_id()).unwrap_or_default();
      let order = server::Order {
        id: Some(id),
        status: Some(order.status.unwrap_or(server::OrderStatus::Placed)),
        complete: Some(order.complete.unwrap_or_default()),
        ..order
      };
      state.orders.insert(id, order.clone());
      Ok(server::ApiResponse::Ok(order))
    })
  }

  async fn get_order_by_id(&self, request: server::GetOrderByIdRequest) -> Response<server::Order, server::Problem> {
    self.record(Received::GetOrderById(request.clone()));
    let id = request.path.order_id;
    self.with_state(|state| match state.orders.get(&id) {
      Some(order) => Ok(server::ApiResponse::Ok(order.clone())),
      None => Ok(server::ApiResponse::NotFound(problem(
        StatusCode::NOT_FOUND,
        format!("no order has id {id}"),
        format!("/store/orders/{id}"),
      ))),
    })
  }

  async fn delete_order(&self, request: server::DeleteOrderRequest) -> Response<(), server::Problem> {
    self.record(Received::DeleteOrder(request.clone()));
    let id = request.path.order_id;
    self.with_state(|state| match state.orders.shift_remove(&id) {
      Some(_) => Ok(server::ApiResponse::NoContent(())),
      None => Ok(server::ApiResponse::NotFound(problem(
        StatusCode::NOT_FOUND,
        format!("no order has id {id}"),
        format!("/store/orders/{id}"),
      ))),
    })
  }

  async fn list_carriers(&self, request: server::ListCarriersRequest) -> Response<server::Carrier200Response> {
    self.record(Received::ListCarriers(request));
    let carriers = self.with_state(|state| {
      if state.carriers.is_empty() {
        default_carriers()
      } else {
        state.carriers.clone()
      }
    });
    Ok(server::ApiResponse::Ok(carriers))
  }

  async fn subscribe_newsletter(&self, request: server::SubscribeNewsletterRequest) -> Response<server::Subscription> {
    self.record(Received::SubscribeNewsletter(request.clone()));
    let signup = request.body;
    let id = self.with_state(State::next_id);
    Ok(server::ApiResponse::Created(server::Subscription {
      id: Uuid::from_u128(u128::from(id)),
      email: signup.email,
      frequency: signup.frequency,
      consent: Some(signup.consent.unwrap_or_default()),
      subscribed_at: Some(now()),
    }))
  }

  async fn unsubscribe_newsletter(&self, request: server::UnsubscribeNewsletterRequest) -> Response<()> {
    self.record(Received::UnsubscribeNewsletter(request));
    Ok(server::ApiResponse::NoContent(()))
  }

  async fn submit_intake_form(&self, request: server::SubmitIntakeFormRequest) -> Response<server::IntakeForm> {
    self.record(Received::SubmitIntakeForm(request.clone()));
    Ok(server::ApiResponse::Ok(request.body))
  }

  async fn upload_media(&self, request: server::UploadMediaRequest) -> Response<server::MediaReceipt> {
    self.record(Received::UploadMedia(request.clone()));
    let body = request.body.unwrap_or_default();
    let size = |bytes: &Vec<u8>| i64::try_from(bytes.len()).unwrap_or_default();
    Ok(server::ApiResponse::Ok(server::MediaReceipt {
      sizes: body.file.iter().flatten().map(size).collect(),
      note: body.note,
      cover_size: body.cover.as_ref().map(size),
    }))
  }

  async fn submit_media_labels(&self, request: server::SubmitMediaLabelsRequest) -> Response<server::MediaLabels> {
    self.record(Received::SubmitMediaLabels(request.clone()));
    Ok(server::ApiResponse::Ok(request.body))
  }

  async fn submit_tallies(&self, request: server::SubmitTalliesRequest) -> Response<server::Tallies> {
    self.record(Received::SubmitTallies(request.clone()));
    Ok(server::ApiResponse::Ok(request.body))
  }

  async fn list_api_keys(
    &self,
    request: server::ListApiKeysRequest,
  ) -> Response<server::Key200Response, server::Error> {
    self.record(Received::ListApiKeys(request.clone()));
    if request.credentials.api_key_auth.expose_secret() != API_KEY {
      return Ok(server::ApiResponse::Unauthorized(error(
        StatusCode::UNAUTHORIZED,
        "unknown API key",
      )));
    }
    let limit = request
      .query
      .limit
      .and_then(|limit| usize::try_from(limit).ok())
      .unwrap_or(usize::MAX);
    let keys = self.with_state(|state| state.api_keys.values().take(limit).cloned().collect());
    Ok(server::ApiResponse::Ok(keys))
  }

  async fn create_api_key(&self, request: server::CreateApiKeyRequest) -> Response<server::ApiKey, server::Error> {
    self.record(Received::CreateApiKey(request.clone()));
    let credentials = &request.credentials;
    if !(exposes(credentials.query_key.as_ref(), QUERY_KEY) || exposes(credentials.session_cookie.as_ref(), SESSION)) {
      return Ok(server::ApiResponse::Unauthorized(error(
        StatusCode::UNAUTHORIZED,
        "unknown query key or session",
      )));
    }
    let key = self.with_state(|state| {
      let key = server::ApiKey {
        id: format!("key_{}", state.next_id()),
        name: request.body.name,
      };
      state.api_keys.insert(key.id.clone(), key.clone());
      key
    });
    Ok(server::ApiResponse::Created(key))
  }

  async fn get_api_key(&self, request: server::GetApiKeyRequest) -> Response<server::ApiKey, server::Error> {
    self.record(Received::GetApiKey(request.clone()));
    let credentials = &request.credentials;
    if credentials.api_key_auth.expose_secret() != API_KEY || credentials.session_cookie.expose_secret() != SESSION {
      return Ok(server::ApiResponse::Unauthorized(error(
        StatusCode::UNAUTHORIZED,
        "the API key and session must both be valid",
      )));
    }
    let key_id = request.path.key_id;
    self.with_state(|state| match state.api_keys.get(&key_id) {
      Some(key) => Ok(server::ApiResponse::Ok(key.clone())),
      None => Ok(server::ApiResponse::Other(
        StatusCode::NOT_FOUND,
        format!("no key has id {key_id}").into_bytes(),
      )),
    })
  }

  async fn replace_api_key(&self, request: server::ReplaceApiKeyRequest) -> Response<server::ApiKey, server::Error> {
    self.record(Received::ReplaceApiKey(request.clone()));
    if let Some(api_key) = &request.credentials.api_key_auth
      && api_key.expose_secret() != API_KEY
    {
      return Ok(server::ApiResponse::Unauthorized(error(
        StatusCode::UNAUTHORIZED,
        "unknown API key",
      )));
    }
    let key = server::ApiKey {
      id: request.path.key_id,
      name: request.body.name,
    };
    self.with_state(|state| state.api_keys.insert(key.id.clone(), key.clone()));
    Ok(server::ApiResponse::Ok(key))
  }

  async fn revoke_api_key(&self, request: server::RevokeApiKeyRequest) -> Response<(), server::Error> {
    self.record(Received::RevokeApiKey(request.clone()));
    let credentials = &request.credentials;
    if !(exposes(credentials.bearer_auth.as_ref(), STAFF_TOKEN) || exposes(credentials.api_key_auth.as_ref(), API_KEY))
    {
      return Ok(server::ApiResponse::Unauthorized(error(
        StatusCode::UNAUTHORIZED,
        "a staff token or API key is required",
      )));
    }
    self.with_state(|state| state.api_keys.shift_remove(&request.path.key_id));
    Ok(server::ApiResponse::NoContent(()))
  }

  async fn list_audit_events(&self, request: server::ListAuditEventsRequest) -> Response<Vec<String>> {
    self.record(Received::ListAuditEvents(request.clone()));
    let credentials = &request.credentials;
    let scheme = if exposes(credentials.bearer_auth.as_ref(), STAFF_TOKEN) {
      "bearer"
    } else if exposes(credentials.token_auth.as_ref(), AUDIT_TOKEN) {
      "token"
    } else {
      return Ok(server::ApiResponse::Other(
        StatusCode::FORBIDDEN,
        b"unrecognized credentials".to_vec(),
      ));
    };
    let events = self.with_state(|state| {
      state
        .received
        .iter()
        .map(|received| {
          format!("{received:?}")
            .split('(')
            .next()
            .unwrap_or_default()
            .to_string()
        })
        .collect::<Vec<_>>()
    });
    Ok(server::ApiResponse::Ok(
      std::iter::once(format!("authenticated by {scheme}"))
        .chain(events)
        .collect(),
    ))
  }

  async fn get_health(&self, request: server::GetHealthRequest) -> Response<()> {
    self.record(Received::GetHealth(request));
    Ok(server::ApiResponse::NoContent(()))
  }

  async fn trace_health(&self, request: server::TraceHealthRequest) -> Response<String> {
    self.record(Received::TraceHealth(request));
    Ok(server::ApiResponse::Ok("TRACE /health".to_string()))
  }

  async fn adopt_pet_status_changed(&self, request: server::AdoptPetStatusChangedRequest) -> Response<()> {
    self.record(Received::AdoptPetStatusChanged(request));
    Ok(server::ApiResponse::NoContent(()))
  }

  async fn pet_adopted(&self, request: server::PetAdoptedRequest) -> Response<()> {
    self.record(Received::PetAdopted(request));
    Ok(server::ApiResponse::Ok(()))
  }

  async fn inventory_low(&self, request: server::InventoryLowRequest) -> Response<()> {
    self.record(Received::InventoryLow(request.clone()));
    Ok(match request.body.validate() {
      Ok(()) => server::ApiResponse::NoContent(()),
      Err(errors) => invalid_webhook(&errors),
    })
  }
}

/// A webhook declares no error response, so a body that breaks its schema gets an
/// undeclared `400` carrying the validation errors.
fn invalid_webhook(errors: &validator::ValidationErrors) -> server::ApiResponse<(), ()> {
  server::ApiResponse::Other(StatusCode::BAD_REQUEST, errors.to_string().into_bytes())
}
