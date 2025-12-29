use serde_json::json;

use crate::fixtures::intersection_union::Vehicle;

#[test]
fn test_valid_case_option_1() {
  // { "id": "123", "wheels": 4 } (Has ID + Matches Option 1)
  let json = json!({ "id": "123", "wheels": 4 });
  let vehicle: Vehicle = serde_json::from_value(json).expect("Should deserialize valid case 1");
  assert_eq!(vehicle.id, "123");
  assert_eq!(vehicle.wheels, Some(4));
}

#[test]
fn test_valid_case_option_2() {
  // { "id": "456", "sails": 2 } (Has ID + Matches Option 2)
  let json = json!({ "id": "456", "sails": 2 });
  let vehicle: Vehicle = serde_json::from_value(json).expect("Should deserialize valid case 2");
  assert_eq!(vehicle.id, "456");
  assert_eq!(vehicle.sails, Some(2));
}

#[test]
fn test_valid_case_both() {
  // { "id": "789", "wheels": 4, "sails": 1 } (Has ID + Matches both—valid for anyOf)
  let json = json!({ "id": "789", "wheels": 4, "sails": 1 });
  let vehicle: Vehicle = serde_json::from_value(json).expect("Should deserialize valid case both");
  assert_eq!(vehicle.id, "789");
  assert_eq!(vehicle.wheels, Some(4));
  assert_eq!(vehicle.sails, Some(1));
}

#[test]
fn test_invalid_fails_allof() {
  // { "wheels": 4 } (Fails allOf — missing id)
  let json = json!({ "wheels": 4 });
  let result = serde_json::from_value::<Vehicle>(json);
  assert!(result.is_err(), "Should fail when missing required field 'id'");
}
