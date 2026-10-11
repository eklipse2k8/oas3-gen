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
