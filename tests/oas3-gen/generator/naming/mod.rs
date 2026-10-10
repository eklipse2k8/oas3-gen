mod identifiers;
mod inference;
mod name_index;
mod operations;
mod responses;

use std::fmt::Debug;

#[track_caller]
fn assert_converts<I, T, E>(convert: impl Fn(I) -> T, cases: &[(I, E)])
where
  I: Copy + Debug,
  T: PartialEq<E> + Debug,
  E: Debug,
{
  for (input, expected) in cases {
    let actual = convert(*input);
    assert!(
      actual == *expected,
      "conversion of {input:?}: expected {expected:?}, got {actual:?}"
    );
  }
}
