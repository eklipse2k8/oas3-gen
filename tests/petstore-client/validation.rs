use chrono::NaiveDate;
use indexmap::IndexSet;
use petstore_client::{self as client, ApiResponse, WithHeaders};
use proptest::{collection, option, prelude::*, sample::select};
use rstest::rstest;
use uuid::Uuid;

use crate::{
  checked, property_config,
  support::{API_VERSION, COMPATIBILITY_DATE, Petstore, now, run_case, start_shared},
};

fn telemetry_with_battery(battery_percent: i32) -> client::CollarTelemetry {
  client::CollarTelemetry {
    collar_id: "COL-0A1B2C3D".to_string(),
    recorded_at: now(),
    location: vec![45.52, -122.68],
    samples: vec![72],
    battery_percent: Some(battery_percent),
    ..Default::default()
  }
}

#[rstest]
#[case::query_range(
  || client::ListPetsRequest::builder().x_compatibility_date(COMPATIBILITY_DATE).limit(101).build().map(drop),
  Some("limit")
)]
#[case::nested_body_multiple_of(
  || client::SubmitCollarTelemetryRequest::builder()
    .collar_id("COL-0A1B2C3D".to_string())
    .body(telemetry_with_battery(87))
    .build()
    .map(drop),
  Some("battery_percent")
)]
#[case::body_pattern(
  || client::CreatePetRequest::builder()
    .body(client::NewPet {
      name: "Rex".to_string(),
      microchip: Some("98511234567890X".to_string()),
      ..Default::default()
    })
    .build()
    .map(drop),
  Some("microchip")
)]
#[case::length_counts_characters_not_bytes(
  || client::SetPetNotesRequest::builder().pet_id(Uuid::nil()).body("\u{e9}".repeat(2000)).build().map(drop),
  None
)]
fn builders_enforce_the_schema_constraints(#[case] build: fn() -> anyhow::Result<()>, #[case] rejected: Option<&str>) {
  match (build(), rejected) {
    (Ok(()), None) => {}
    (Err(error), Some(field)) => assert!(format!("{error:#}").contains(field), "`{error:#}` names {field}"),
    (built, rejected) => panic!("expected {rejected:?} to be rejected, got {built:?}"),
  }
}

fn species() -> impl Strategy<Value = client::Species> {
  select(vec![
    client::Species::Dog,
    client::Species::Cat,
    client::Species::Bird,
    client::Species::Fish,
    client::Species::Reptile,
    client::Species::SmallMammal,
  ])
}

prop_compose! {
  fn valid_new_pet()(
    name in "[A-Za-z][A-Za-z '-]{0,63}",
    species in species(),
    nickname in option::of("[A-Za-z]{1,16}"),
    birth_date in option::of((728_000..=740_000i32).prop_filter_map("a calendar day", NaiveDate::from_num_days_from_ce_opt)),
    weight_kg in option::of((1..=500_000u32).prop_map(|grams| f64::from(grams) / 1000.0)),
    microchip in option::of("[0-9]{15}"),
    tags in option::of(collection::vec("[a-z]{1,10}", 0..=10)),
  ) -> client::NewPet {
    client::NewPet {
      name,
      species,
      nickname,
      birth_date,
      weight_kg,
      microchip,
      tags: tags.map(|tags| tags.into_iter().collect::<IndexSet<_>>()),
    }
  }
}

async fn reads_back_unchanged(petstore: &Petstore, pet: client::NewPet) -> anyhow::Result<()> {
  let create = client::CreatePetRequest::builder().body(pet.clone()).build()?;
  let created =
    answered!(petstore.client.create_pet(create).await?, ApiResponse::Created(WithHeaders { body, .. }) => body);
  let show = client::ShowPetByIdRequest::builder()
    .pet_id(created.id)
    .x_api_version(API_VERSION.to_string())
    .build()?;
  let shown = answered!(petstore.client.show_pet_by_id(show).await?, ApiResponse::Ok(WithHeaders { body, .. }) => body);
  let read = client::NewPet {
    name: shown.name,
    species: shown.species,
    nickname: shown.nickname,
    birth_date: shown.birth_date,
    weight_kg: shown.weight_kg,
    microchip: shown.microchip,
    tags: shown.tags,
  };
  anyhow::ensure!(read == pet, "read back {read:?}");
  anyhow::ensure!(
    read.tags.iter().flatten().eq(pet.tags.iter().flatten()),
    "tags kept their order"
  );
  Ok(())
}

#[rstest]
fn any_new_pet_within_the_schema_builds_and_reads_back_unchanged() {
  let petstore = start_shared();
  proptest!(property_config(48), |(pet in valid_new_pet())| {
    checked(run_case(async || reads_back_unchanged(&petstore, pet).await))?;
  });
}
