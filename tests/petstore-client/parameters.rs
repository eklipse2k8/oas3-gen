use chrono::NaiveDate;
use indexmap::IndexSet;
use petstore_client::{self as client, ApiResponse};
use petstore_server as server;
use proptest::{collection, option, prelude::*, sample::select};
use rstest::rstest;
use serde_json::json;
use uuid::Uuid;

use crate::{
  Wiretap, checked, property_config,
  support::{COMPATIBILITY_DATE, Petstore, PetstoreService, SESSION, run_case, start_shared},
};

enum Query {
  List(client::ListPetsRequestQuery),
  Search(client::SearchPetsRequestQuery),
}

fn search(query: client::SearchPetsRequestQuery) -> Query {
  Query::Search(client::SearchPetsRequestQuery {
    q: "resident".to_string(),
    ..query
  })
}

fn biscuit() -> server::Pet {
  server::Pet {
    id: Uuid::from_u128(0xB15C),
    name: "Biscuit".to_string(),
    species: server::Species::Dog,
    ..Default::default()
  }
}

#[rstest]
#[case::form_exploded(
  search(client::SearchPetsRequestQuery {
    tags: Some(vec!["calm".to_string(), "salt & pepper".to_string()]),
    statuses: Some(vec![client::PetStatus::Pending, client::PetStatus::Adopted]),
    born_in: Some(vec![2019, 2022]),
    ..Default::default()
  }),
  "/pets/search?q=resident&tags=calm&tags=salt+%26+pepper&statuses=pending&statuses=adopted&bornIn=2019&bornIn=2022"
)]
#[case::form_joined(
  Query::List(client::ListPetsRequestQuery {
    limit: Some(5),
    status: Some(client::PetStatus::Available),
    tags: Some(vec!["friendly".to_string(), "small".to_string()]),
    ..Default::default()
  }),
  "/pets?limit=5&status=available&tags=friendly%2Csmall"
)]
#[case::space_and_pipe_delimited(
  search(client::SearchPetsRequestQuery {
    species: Some(vec!["dog".to_string(), "cat".to_string()]),
    colors: Some(vec!["brown".to_string(), "white".to_string()]),
    ..Default::default()
  }),
  "/pets/search?q=resident&species=dog+cat&colors=brown%7Cwhite"
)]
#[case::scalars_and_reserved_characters(
  Query::Search(client::SearchPetsRequestQuery {
    q: "salt & pepper = 100% cute+ #1 [a/b]:@!$'()*,;".to_string(),
    filter: Some(json!("coat:short")),
    min_weight_kg: Some(2.5),
    vaccinated: Some(true),
    born_after: NaiveDate::from_ymd_opt(2020, 1, 31),
    ..Default::default()
  }),
  "/pets/search?q=salt+%26+pepper+%3D+100%25+cute%2B+%231+%5Ba%2Fb%5D%3A%40%21%24%27%28%29*%2C%3B&filter=coat%3Ashort&minWeightKg=2.5&vaccinated=true&bornAfter=2020-01-31"
)]
#[tokio::test]
async fn query_parameters_are_encoded_in_their_declared_style(
  #[case] query: Query,
  #[case] request_target: &str,
) -> anyhow::Result<()> {
  let wiretap = Wiretap::petstore().await?;

  match query {
    Query::List(query) => {
      let header = client::ListPetsRequestHeader {
        x_compatibility_date: COMPATIBILITY_DATE,
        ..Default::default()
      };
      let request = client::ListPetsRequest { query, header };
      answered!(wiretap.client().list_pets(request).await?, ApiResponse::Ok(_) => ());
    }
    Query::Search(query) => {
      let request = client::SearchPetsRequest { query };
      answered!(wiretap.client().search_pets(request).await?, ApiResponse::Ok(_) => ());
    }
  }

  assert_eq!(wiretap.last()?.uri, request_target);
  Ok(())
}

#[rstest]
#[tokio::test]
async fn header_parameters_are_sent_in_simple_style() -> anyhow::Result<()> {
  let wiretap = Wiretap::petstore().await?;
  let request_id = Uuid::from_u128(0x0192_f0c1_7a6e_7c4e_9d2a_5b1e_8f3c_2a10);
  let request = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .x_only(vec![client::Species::Cat, client::Species::SmallMammal])
    .x_sort_order(client::ListPetsRequestHeaderXSortOrder::Desc)
    .x_request_id(request_id)
    .build()?;

  let page = answered!(wiretap.client().list_pets(request).await?, ApiResponse::Ok(page) => page);

  let wire = wiretap.last()?;
  let sent = ["x-only", "x-sort-order", "x-request-id", "x-compatibility-date"].map(|name| wire.header(name));
  assert_eq!(
    sent,
    [
      Some("cat,small_mammal"),
      Some("desc"),
      Some("0192f0c1-7a6e-7c4e-9d2a-5b1e8f3c2a10"),
      Some("2026-06-09")
    ]
  );
  let received = received!(wiretap.petstore.service, ListPets).header;
  assert_eq!(
    received.x_only,
    Some(vec![server::Species::Cat, server::Species::SmallMammal])
  );
  assert_eq!(
    received.x_sort_order,
    Some(server::ListPetsRequestHeaderXSortOrder::Desc)
  );
  assert_eq!(received.x_compatibility_date, COMPATIBILITY_DATE);
  assert_eq!(page.headers.x_request_id, Some(request_id));
  Ok(())
}

enum Stored {
  Nothing,
  Session,
  CardTheme,
}

async fn card_store(stored: Stored) -> anyhow::Result<Wiretap> {
  let service = PetstoreService::default();
  let router = petstore_server::router(service.clone()).route(
    "/stored-theme",
    axum::routing::get(|| async { ([(http::header::SET_COOKIE, "cardTheme=dark; Path=/")], "stored") }),
  );
  let mut wiretap = Wiretap::serve(router, service).await?;
  match stored {
    Stored::Nothing => {}
    Stored::Session => {
      wiretap.petstore.client = wiretap.petstore.client.clone().with_session_cookie(SESSION);
    }
    Stored::CardTheme => {
      wiretap
        .client()
        .client
        .get(wiretap.petstore.url("/stored-theme"))
        .send()
        .await?;
    }
  }
  wiretap.requests();
  Ok(wiretap)
}

#[rstest]
#[case::parameters_in_declaration_order(
  Stored::Nothing,
  client::GetPetCardRequestCookie {
    card_theme: Some(client::GetPetCardRequestCookieCardTheme::HighContrast),
    recently_viewed: Some(vec![Uuid::from_u128(1), Uuid::from_u128(2)]),
    pinned_pets: Some(IndexSet::new()),
  },
  "cardTheme=high-contrast; recentlyViewed=00000000-0000-0000-0000-000000000001%2C00000000-0000-0000-0000-000000000002; pinnedPets=",
  "high-contrast"
)]
#[case::stored_session_follows_the_parameters(
  Stored::Session,
  client::GetPetCardRequestCookie {
    card_theme: Some(client::GetPetCardRequestCookieCardTheme::Dark),
    ..Default::default()
  },
  "cardTheme=dark; session=session-2026",
  "dark"
)]
#[case::parameter_shadows_a_stored_cookie_of_its_name(
  Stored::CardTheme,
  client::GetPetCardRequestCookie {
    card_theme: Some(client::GetPetCardRequestCookieCardTheme::Light),
    ..Default::default()
  },
  "cardTheme=light",
  "light"
)]
#[tokio::test]
async fn cookie_parameters_travel_in_one_cookie_header(
  #[case] stored: Stored,
  #[case] cookie: client::GetPetCardRequestCookie,
  #[case] cookie_header: &str,
  #[case] card_theme: &str,
) -> anyhow::Result<()> {
  let wiretap = card_store(stored).await?;
  let pet_id = wiretap.petstore.service.insert_pet(biscuit());
  let request = client::GetPetCardRequest {
    path: client::GetPetCardRequestPath { pet_id },
    cookie: cookie.clone(),
  };

  answered!(wiretap.client().get_pet_card(request).await?, ApiResponse::Ok(_) => ());

  assert_eq!(wiretap.last()?.header(http::header::COOKIE), Some(cookie_header));
  let received = received!(wiretap.petstore.service, GetPetCard).cookie;
  assert_eq!(
    received.card_theme.map(|theme| theme.to_string()).as_deref(),
    Some(card_theme)
  );
  assert_eq!(received.recently_viewed, cookie.recently_viewed);
  assert_eq!(received.pinned_pets, cookie.pinned_pets);
  Ok(())
}

fn pet_status() -> impl Strategy<Value = (client::PetStatus, server::PetStatus)> {
  select(vec![
    (client::PetStatus::Available, server::PetStatus::Available),
    (client::PetStatus::Pending, server::PetStatus::Pending),
    (client::PetStatus::Adopted, server::PetStatus::Adopted),
  ])
}

fn card_theme() -> impl Strategy<Value = client::GetPetCardRequestCookieCardTheme> {
  select(vec![
    client::GetPetCardRequestCookieCardTheme::Light,
    client::GetPetCardRequestCookieCardTheme::Dark,
    client::GetPetCardRequestCookieCardTheme::HighContrast,
  ])
}

fn non_empty<T>(items: Vec<T>) -> Option<Vec<T>> {
  (!items.is_empty()).then_some(items)
}

#[derive(Debug)]
struct Parameters {
  q: String,
  tags: Vec<String>,
  statuses: Vec<(client::PetStatus, server::PetStatus)>,
  born_in: Vec<i32>,
  listed_tags: Vec<String>,
  card_theme: Option<client::GetPetCardRequestCookieCardTheme>,
  recently_viewed: Option<Vec<Uuid>>,
}

async fn parameters_arrive(petstore: &Petstore, pet_id: Uuid, parameters: Parameters) -> anyhow::Result<()> {
  let (statuses, expected_statuses) = parameters.statuses.into_iter().unzip::<_, _, Vec<_>, Vec<_>>();
  let search = client::SearchPetsRequest::builder()
    .q(parameters.q.clone())
    .tags(parameters.tags.clone())
    .statuses(statuses)
    .born_in(parameters.born_in.clone())
    .build()?;
  answered!(petstore.client.search_pets(search).await?, ApiResponse::Ok(_) => ());
  let received = received!(petstore.service, SearchPets).query;
  anyhow::ensure!(received.q == parameters.q, "q arrived as {:?}", received.q);
  anyhow::ensure!(
    received.tags == non_empty(parameters.tags),
    "tags arrived as {:?}",
    received.tags
  );
  anyhow::ensure!(
    received.statuses == non_empty(expected_statuses),
    "statuses arrived as {:?}",
    received.statuses
  );
  anyhow::ensure!(
    received.born_in == non_empty(parameters.born_in),
    "bornIn arrived as {:?}",
    received.born_in
  );

  let list = client::ListPetsRequest::builder()
    .x_compatibility_date(COMPATIBILITY_DATE)
    .tags(parameters.listed_tags.clone())
    .build()?;
  answered!(petstore.client.list_pets(list).await?, ApiResponse::Ok(_) => ());
  let received = received!(petstore.service, ListPets).query;
  anyhow::ensure!(
    received.tags == Some(parameters.listed_tags),
    "tags arrived as {:?}",
    received.tags
  );

  let card = client::GetPetCardRequest::builder()
    .pet_id(pet_id)
    .maybe_card_theme(parameters.card_theme.clone())
    .maybe_recently_viewed(parameters.recently_viewed.clone())
    .build()?;
  answered!(petstore.client.get_pet_card(card).await?, ApiResponse::Ok(_) => ());
  let received = received!(petstore.service, GetPetCard).cookie;
  anyhow::ensure!(
    received.card_theme.map(|theme| theme.to_string()) == parameters.card_theme.map(|theme| theme.to_string()),
    "cardTheme arrived wrong"
  );
  anyhow::ensure!(
    received.recently_viewed == parameters.recently_viewed,
    "recentlyViewed arrived as {:?}",
    received.recently_viewed
  );
  Ok(())
}

prop_compose! {
  fn parameters()(
    q in "\\PC{1,24}",
    tags in collection::vec("\\PC{1,12}", 0..4),
    statuses in collection::vec(pet_status(), 0..4),
    born_in in collection::vec(1990..=2026i32, 0..4),
    listed_tags in collection::vec("[a-z][a-z ]{0,7}", 1..4),
    card_theme in option::of(card_theme()),
    recently_viewed in option::of(collection::vec(any::<u128>().prop_map(Uuid::from_u128), 1..=5)),
  ) -> Parameters {
    Parameters { q, tags, statuses, born_in, listed_tags, card_theme, recently_viewed }
  }
}

#[rstest]
fn any_parameter_values_arrive_as_the_client_built_them() {
  let petstore = start_shared();
  let pet_id = petstore.service.insert_pet(biscuit());
  proptest!(property_config(48), |(parameters in parameters())| {
    checked(run_case(async || parameters_arrive(&petstore, pet_id, parameters).await))?;
  });
}
