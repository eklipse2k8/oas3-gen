use std::sync::LazyLock;

use regex::Regex;
use rstest::rstest;

use crate::{assert_contains_all, generate_types, make_orchestrator, parse_spec};

static TYPE_DECLARATION: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?m)^pub (?:enum|struct|type) (\w+)").expect("type declaration pattern compiles"));

#[rstest]
fn closed_enums_keep_schema_names_and_shared_inline_schemas_share_one_type() {
  let orchestrator = make_orchestrator(parse_spec(spec!("naming/type_names.json")), true);
  let code = generate_types(&orchestrator, "type_names.json").code;

  let declared = TYPE_DECLARATION
    .captures_iter(&code)
    .map(|captures| captures[1].to_string())
    .collect::<Vec<_>>();
  assert_eq!(
    declared,
    [
      "VoiceIdsShared",
      "FormatType",
      "Status",
      "ChatModel",
      "ModelIdsShared",
      "ChatRequest",
      "CompletionRequest",
      "Owner",
      "Clinic",
      "RequestMaxOutputTokens",
      "ChatRequestStatus",
      "ChatRequestVoice",
      "ChatRequestFormat",
      "OwnerAddress",
    ]
  );
  assert_contains_all(
    &code,
    &[
      (
        "pub max_output_tokens: Option<RequestMaxOutputTokens>,",
        "union shared by two requests",
      ),
      (
        "pub status: Option<ChatRequestStatus>,",
        "inline enum named after parent and property",
      ),
      (
        "pub voice: Option<ChatRequestVoice>,",
        "closed inline enum not named after an open union",
      ),
      (
        "pub format: Option<ChatRequestFormat>,",
        "closed inline enum not named after const union",
      ),
      ("pub state: Option<Status>,", "inline enum reusing a closed schema enum"),
      ("pub address: Option<OwnerAddress>,", "object shared by two parents"),
      ("pub location: Option<OwnerAddress>,", "object shared by two parents"),
    ],
  );
}
