use std::collections::BTreeSet;

use oas3_gen::generator::naming::responses::{headers_name, union_member_name, union_name};
use rstest::rstest;

use super::assert_converts;

fn taken(names: &[&str]) -> BTreeSet<String> {
  names.iter().map(ToString::to_string).collect()
}

#[rstest]
fn response_header_and_union_types_are_named_after_their_parts() {
  assert_converts(
    |(headers, used): (&[&str], &[&str])| headers_name(headers.iter().copied(), &taken(used)),
    &[
      ((&["x-next"][..], &[][..]), "XNextHeaders"),
      ((&["Link", "X-Total-Count"], &[]), "LinkAndXTotalCountHeaders"),
      ((&["Link"], &["LinkHeaders"]), "LinkHeaders2"),
      ((&["Link"], &["LinkHeaders", "LinkHeaders2"]), "LinkHeaders3"),
    ],
  );
  assert_converts(
    union_member_name,
    &[
      ("Pet", "Pet"),
      ("oas3_gen_support::EventStream<Pet>", "PetStream"),
      (
        "oas3_gen_support::EventStream<oas3_gen_support::SseFrame<Pet>>",
        "PetStream",
      ),
      (
        "futures::stream::BoxStream<'static, AssistantEventFrame>",
        "AssistantEventFrameStream",
      ),
      ("Vec<u8>", "Vecu8"),
    ],
  );
  assert_converts(
    |(members, used): (&[&str], &[&str])| union_name(members.iter().copied(), &taken(used)),
    &[
      (
        (
          &["ContentResponse", "oas3_gen_support::EventStream<AssistantEventFrame>"][..],
          &[][..],
        ),
        "ContentResponseOrAssistantEventFrameStream",
      ),
      ((&["Pet", "Error"], &["PetOrError"]), "PetOrErrorBody"),
      (
        (&["Pet", "Error"], &["PetOrError", "PetOrErrorBody"]),
        "PetOrErrorBody2",
      ),
    ],
  );
}
