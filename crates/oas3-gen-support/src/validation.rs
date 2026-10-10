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

/// The number field types [`multiple_of_ratio`] checks, `f32` and `f64`. The trait is sealed.
pub trait MultipleOfFloat: Copy + Into<f64> + PartialEq + sealed::Sealed {}

impl MultipleOfFloat for f32 {}

impl MultipleOfFloat for f64 {}

mod sealed {
  pub trait Sealed: Sized {
    /// Rounds `value` to the nearest number of this type.
    fn round_to_type(self, value: f64) -> Self;
  }

  impl Sealed for f32 {
    #[expect(
      clippy::cast_possible_truncation,
      reason = "the nearest f32 is the point of the rounding"
    )]
    fn round_to_type(self, value: f64) -> Self {
      value as f32
    }
  }

  impl Sealed for f64 {
    fn round_to_type(self, value: f64) -> Self {
      value
    }
  }
}

/// Checks JSON Schema `multipleOf` for a number field, where the multiple is the fraction
/// `NUMERATOR / DENOMINATOR`, such as `1 / 4` for `multipleOf: 0.25`.
///
/// The value passes when the nearest multiple, rounded to the value's own type, is the value
/// itself, or when the value divided by the fraction is within `1e-9` of a whole number, which
/// absorbs arithmetic rounding such as `0.1 + 0.2`. Where the type's spacing is coarser than the
/// multiple, every value rounds to a multiple and passes.
///
/// ```
/// assert!(oas3_gen_support::multiple_of_ratio::<1, 4>(2.75_f64).is_ok());
/// assert!(oas3_gen_support::multiple_of_ratio::<1, 10>(0.3_f32).is_ok());
/// assert!(oas3_gen_support::multiple_of_ratio::<1, 4>(2.8_f32).is_err());
/// ```
///
/// # Errors
///
/// Returns a `multiple_of` error when `value` isn't a multiple of the fraction.
pub fn multiple_of_ratio<const NUMERATOR: u32, const DENOMINATOR: u32>(
  value: impl MultipleOfFloat,
) -> Result<(), ValidationError> {
  let quotient = value.into() * f64::from(DENOMINATOR) / f64::from(NUMERATOR);
  let whole = quotient.round();
  let nearest_multiple = whole * f64::from(NUMERATOR) / f64::from(DENOMINATOR);
  let exact_at_type = value == value.round_to_type(nearest_multiple);
  let within_tolerance = (quotient - whole).abs() <= 1e-9;
  if NUMERATOR != 0 && quotient.is_finite() && (exact_at_type || within_tolerance) {
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
  use std::{any::type_name, fmt::Display};

  use super::{ValidationError, multiple_of, multiple_of_ratio};

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
      "0.3 is the f64 nearest to 3 tenths"
    );
    assert!(multiple_of_ratio::<3, 2>(4.5_f32).is_ok(), "4.5 is a multiple of 1.5");
  }

  fn assert_multiples<T: Copy + Display>(
    check: fn(T) -> Result<(), ValidationError>,
    multiple: &str,
    cases: &[(T, bool)],
  ) {
    for &(value, valid) in cases {
      assert_eq!(
        check(value).is_ok(),
        valid,
        "{value} ({}) as a multiple of {multiple}",
        type_name::<T>()
      );
    }
  }

  #[test]
  fn f32_multiples_are_checked_at_f32_precision() {
    assert_multiples::<f32>(
      multiple_of_ratio::<1, 10>,
      "0.1",
      &[
        (0.0, true),
        (-0.0, true),
        (0.1, true),
        (0.3, true),
        (-0.3, true),
        (0.7, true),
        (12345.6, true),
        (-12345.6, true),
        (1_000_000.5, true),
        (2_000_000_000.0, true),
        (0.35, false),
        (-0.35, false),
        (0.05, false),
        (1e-7, false),
        (f32::NAN, false),
        (f32::INFINITY, false),
        (1_100_000.1, true),
        (1_000_000.44, false),
        (1_000_000.56, false),
        (300_000.03, false),
      ],
    );
    assert_multiples::<f32>(
      multiple_of_ratio::<1, 4>,
      "0.25",
      &[
        (0.25, true),
        (-2.75, true),
        (1_000_000.25, true),
        (0.1, false),
        (-0.6, false),
        (1_000_000.06, false),
        (2_500_000.8, true),
      ],
    );
    assert_multiples::<f32>(
      multiple_of_ratio::<1, 1>,
      "1",
      &[
        (2_000_000_000.0, true),
        (1_000_000.5, false),
        (-3.5, false),
        (1_000_000.2, false),
        (2_097_152.2, false),
        (2_097_153.0, true),
        (2_500_000.5, false),
        (-2_500_000.5, false),
      ],
    );
    assert_multiples::<f32>(
      multiple_of_ratio::<1, 100>,
      "0.01",
      &[(25_000.01, true), (25_000.004, false)],
    );
    assert_multiples::<f32>(
      multiple_of_ratio::<2, 1>,
      "2",
      &[
        (4_000_000.5, false),
        (4_000_000.2, false),
        (4_000_001.0, false),
        (4_000_002.0, true),
      ],
    );
    assert_multiples::<f32>(
      multiple_of_ratio::<1, 2>,
      "0.5",
      &[(2_097_152.5, true), (2_097_152.2, false)],
    );
  }

  #[test]
  fn f64_multiples_hold_at_any_magnitude() {
    let accumulated: f64 = std::iter::repeat_n(0.1_f64, 100).sum();
    let accumulated_hundredths: f64 = std::iter::repeat_n(0.01_f64, 1000).sum();
    assert_multiples::<f64>(
      multiple_of_ratio::<1, 10>,
      "0.1",
      &[
        (0.0, true),
        (0.1, true),
        (0.3, true),
        (0.1 + 0.2, true),
        (accumulated, true),
        (-0.3, true),
        (12345.6, true),
        (250_000_000.1, true),
        (1e15, true),
        (0.35, false),
        (0.05, false),
        (1e-7, false),
      ],
    );
    assert_multiples::<f64>(
      multiple_of_ratio::<1, 100>,
      "0.01",
      &[(accumulated_hundredths, true), (0.07, true), (0.075, false)],
    );
    assert_multiples::<f64>(
      multiple_of_ratio::<1, 4>,
      "0.25",
      &[
        (250_000_000.25, true),
        (-2.75, true),
        (250_000_000.1, false),
        (1_000_000_000.1, false),
      ],
    );
    assert_multiples::<f64>(
      multiple_of_ratio::<1, 1>,
      "1",
      &[
        (2_000_000_000.0, true),
        (9_007_199_254_740_994.0, true),
        (2_000_000_000.3, false),
        (250_000_000.1, false),
        (1e15 + 0.5, false),
      ],
    );
  }
}
