use oas3_gen_support::{multiple_of, multiple_of_ratio};
use proptest::prelude::*;

use crate::property_config;

fn not_finite() -> impl Strategy<Value = f64> {
  prop_oneof![Just(f64::NAN), Just(f64::INFINITY), Just(f64::NEG_INFINITY)]
}

proptest! {
  #![proptest_config(property_config(256))]

  #[test]
  fn multiples_pass_and_values_between_multiples_fail(
    whole in -1_000_000i32..1_000_000,
    remainder in 1i64..5,
    steps in 0usize..1000,
    f32_whole in -16_777_216i32..=16_777_216,
    not_finite in not_finite(),
  ) {
    prop_assert!(multiple_of::<5>(i64::from(whole) * 5).is_ok());
    prop_assert!(multiple_of::<5>(i64::from(whole) * 5 + remainder).is_err());
    prop_assert!(multiple_of::<0>(whole).is_err());

    let accumulated = std::iter::repeat_n(0.1_f64, steps).sum::<f64>();
    prop_assert!(multiple_of_ratio::<1, 10>(f64::from(whole) / 10.0).is_ok());
    prop_assert!(multiple_of_ratio::<1, 10>(accumulated).is_ok(), "{} sums {} tenths", accumulated, steps);
    prop_assert!(multiple_of_ratio::<1, 10>((f64::from(whole) + 0.5) / 10.0).is_err());
    prop_assert!(multiple_of_ratio::<1, 10>(not_finite).is_err());

    #[expect(clippy::cast_precision_loss, reason = "whole numbers up to 2^24 are exact in f32")]
    let (whole, f32_whole) = (whole as f32, f32_whole as f32);
    prop_assert!(multiple_of_ratio::<1, 10>(f32_whole / 10.0).is_ok(), "{} is the f32 nearest a tenth", f32_whole / 10.0);
    prop_assert!(multiple_of_ratio::<1, 4>((whole + 0.5) / 4.0).is_err());
  }
}
