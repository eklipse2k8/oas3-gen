# Agent Guidance

Written for Claude Opus 5 and later, following Anthropic's
[Opus 5 prompting guidance](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-opus-5).
Two levers matter here: **scope** (one converter change rewrites thousands of fixture lines) and
**no redundant verification** (the model already self-verifies; instructing it again only burns
tokens).

## Scope

- Deliver what was asked, at the intended scope. Make routine calls yourself; ask only when
  readings of the request diverge into materially different work.
- Finish completely. No stubs, `todo!()`, or unhandled match arms in the pipeline.
- If the request looks mistaken, say so in a sentence and continue as asked — don't silently
  narrow, widen, or transform it.
- Don't bump dependencies, restructure modules, or change public API shape unless that was the task.

## Verification

`cargo test`, `cargo clippy`, and the fixture diff *are* the verification step.

- Don't add a second review pass, and don't delegate a double-check.
- Don't re-read a file to confirm an edit landed.
- Report faithfully: failing tests get quoted; a skipped fixture rebuild gets said out loud.

## Grounding

Retrieval over recall for every Rust question. Pinned crate versions live in the root
`Cargo.toml` and move faster than training data.

- Never describe code you haven't opened.
- Verify generated-output claims against `crates/oas3-gen/fixtures/`.

## Delegation

Work here is mostly sequential edits through one pipeline, so delegation rarely pays.

- Subagents only for large, genuinely parallel investigation (e.g. sweeping `converter/` and
  `codegen/` at once). One is usually enough.
- Never to verify your own work, never for anything a handful of tool calls would finish.

## Communication

- One sentence before the first tool call; updates only for findings or direction changes.
- Lead with the outcome when done.
- Effort controls thinking, not output length — brevity has to be chosen deliberately.
- Correct an earlier statement only when the error changes code, conclusions, or decisions.
- Don't write summary, plan, or report files unless asked. Match document length to substance.

## Effort

| Work | Effort |
|---|---|
| Docs, single-fragment tweaks, tests, fixture rebuilds | `low` |
| Localized converter/codegen changes, bugs with a known repro | `medium` |
| Multi-file features, pipeline refactors, new generation modes | `high` (default) |
| AST or type-resolution redesign | `xhigh` |

For review and bug-finding, ask for everything and filter afterwards. "Only high-severity"
gets obeyed literally and suppresses real findings.
