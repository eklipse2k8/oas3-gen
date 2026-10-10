use std::borrow::Cow;

use validator::ValidationError;

/// Checks JSON Schema `multipleOf` for an integer field: `value` must divide evenly by `N`.
///
/// Generated types call it through `#[validate(custom(function = ...))]`, which passes number
/// fields by value, so the constraint is checked with the rest of a type's validation.
///
/// ```
/// assert!(oas3_gen_support::multiple_of::<5>(85_i32).is_ok());
/// assert!(oas3_gen_support::multiple_of::<5>(87_i64).is_err());
/// ```
///
/// # Errors
///
/// Returns a `multiple_of` error when `value` isn't a multiple of `N`.
pub fn multiple_of<const N: u64>(value: impl Into<i128>) -> Result<(), ValidationError> {
  let value = value.into();
  let divisor = i128::from(N);
  if divisor != 0 && value % divisor == 0 {
    Ok(())
  } else {
    Err(multiple_of_error(&N.to_string()))
  }
}

/// Checks JSON Schema `multipleOf` for a number field, where the multiple is the fraction
/// `NUMERATOR / DENOMINATOR`, such as `1 / 4` for `multipleOf: 0.25`.
///
/// The quotient may differ from a whole number by floating-point rounding, which is tolerated.
///
/// ```
/// assert!(oas3_gen_support::multiple_of_ratio::<1, 4>(2.75_f64).is_ok());
/// assert!(oas3_gen_support::multiple_of_ratio::<1, 4>(2.8_f32).is_err());
/// ```
///
/// # Errors
///
/// Returns a `multiple_of` error when `value` isn't a multiple of the fraction.
pub fn multiple_of_ratio<const NUMERATOR: u32, const DENOMINATOR: u32>(
  value: impl Into<f64>,
) -> Result<(), ValidationError> {
  let quotient = value.into() * f64::from(DENOMINATOR) / f64::from(NUMERATOR);
  let tolerance = 1e-9 * quotient.abs().max(1.0);
  if NUMERATOR != 0 && quotient.is_finite() && (quotient - quotient.round()).abs() <= tolerance {
    Ok(())
  } else {
    Err(multiple_of_error(&format!("{NUMERATOR}/{DENOMINATOR}")))
  }
}

fn multiple_of_error(multiple: &str) -> ValidationError {
  let mut error = ValidationError::new("multiple_of");
  error.add_param(Cow::Borrowed("multiple_of"), &multiple);
  error
}

#[cfg(test)]
mod tests {
  use super::{multiple_of, multiple_of_ratio};

  #[test]
  fn integers_must_divide_evenly() {
    let cases = [(0_i64, true), (85, true), (-15, true), (87, false), (i64::MIN, false)];
    for (value, valid) in cases {
      assert_eq!(multiple_of::<5>(value).is_ok(), valid, "{value} as a multiple of 5");
    }
    assert!(multiple_of::<0>(0_i32).is_err(), "nothing is a multiple of 0");
    assert!(multiple_of::<7>(u64::MAX).is_err(), "u64::MAX isn't a multiple of 7");
  }

  #[test]
  fn numbers_tolerate_rounding() {
    let cases = [
      (0.75_f64, true),
      (2.25, true),
      (0.3, false),
      (-1.5, true),
      (f64::NAN, false),
    ];
    for (value, valid) in cases {
      assert_eq!(
        multiple_of_ratio::<1, 4>(value).is_ok(),
        valid,
        "{value} as a multiple of 0.25"
      );
    }
    assert!(
      multiple_of_ratio::<1, 10>(0.3_f64).is_ok(),
      "0.1 + 0.2 rounding is tolerated"
    );
    assert!(multiple_of_ratio::<3, 2>(4.5_f32).is_ok(), "4.5 is a multiple of 1.5");
  }
}
