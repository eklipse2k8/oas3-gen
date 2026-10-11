---
name: slop
description: This skill should be used when the user asks to "remove slop", "deslop", "clean up slop", "clean up the diff", "strip AI slop from this change", or runs /slop after a change, to remove noise from changed oas3-gen Rust code while preserving behavior. Accepts an optional path, commit, or revision range to clean instead of the uncommitted changes.
argument-hint: "[path | commit | range]"
---

# Get Rid of Slop

Edit the changed Rust code to remove slop: code and comments that a careful
maintainer of the file would not have written. Preserve behavior.

## Find the Changes

1. If arguments name a commit or revision range, clean that change:
   `git show <commit>` or `git diff <range>`. If they name a path, clean its
   uncommitted diff (`git diff HEAD -- <path>`), or the whole file when it has
   none.
2. Otherwise, if `git diff HEAD` touches Rust files, clean the uncommitted work.
3. Otherwise clean the branch: `git diff main...HEAD`.

Edit the code as it stands at HEAD, and only where the change introduced or
modified it. Skip files the change touched that no longer exist, and skip the
generated crates under `fixtures/`, which regeneration rewrites. Read the code
around each change; the untouched code in the file sets the baseline for what
is normal there.

When the change touches `crates/oas3-gen/src/`, snapshot the generated output
before the first edit:

```bash
.claude/skills/slop/scripts/snapshot.sh /tmp/slop-before
```

## Slop to Remove

### Comments

- Inline comments (`//`) in changed code. The project allows none. Rename the
  function or its variables so the code says what the comment said, then delete
  the comment.
- Rustdoc that restates an item's name or signature. Rename instead. Rustdoc
  that records a decision or behavior the code can't state stays.
- Comments a human wouldn't add, or that are inconsistent with the rest of the
  file.

`pub` and `pub(crate)` items in `crates/oas3-gen` are not public API. They are
visible only so the external `tests/` package can reach generator internals.
Don't add rustdoc to them or preserve their signatures for that reason alone.
`oas3-gen-support` is the public runtime API that generated code depends on,
and its rustdoc stays.

### Structure

- A free function added on its own in a file: consider moving it into the
  `impl` block of the type it serves.
- A function with one caller: consider whether it needs to exist, or whether
  its body reads better at the call site. Callers in `tests/` count.
- Wrappers, indirection, traits, or helper functions without clear value.
- One shape repeated several times in the change: consider a `macro_rules!`
  that states the shape once, applied at each site. A macro with a single use
  is itself slop.

### Rust 2024 Idiom

The workspace uses edition 2024 with MSRV 1.89.

- Prefer functional, idempotent, side-effect-free code: iterator chains over
  `mut` accumulators and push loops, without nesting an iterator or allocating
  inside a per-element closure. Free functions stay pure; mutable state lives
  behind `&mut self`.
- Flatten nested `if let` into let chains (`if let Some(x) = a && let Ok(y) =
  b`), and use `let-else` for early returns.
- Remove `.clone()` calls where a borrow compiles.

### Defensive Code

- Checks or error handling abnormal for that area of the codebase, especially
  on trusted or already-validated paths: `match`, `if let`, `.ok()`, or
  `.unwrap_or_default()` that swallows a case that can't occur, re-validation of
  data the parser already checked, and `Result` returns from functions that
  can't fail. Validation belongs at boundaries only: CLI input and the parsed
  spec. A `match` arm that exhaustiveness requires stays.

### Anything Else

- Any other style inconsistent with the file or the project conventions.

## Guidelines

- Keep the change minimal: touch only what removing slop requires.
- Don't rewrite or reformat code unless the cleanup requires it.
- Prefer deleting code over adding abstractions.
- When intent is unclear, infer it from the surrounding code and keep behavior
  the same.

## Verify

1. When `/tmp/slop-before` was taken at the start, snapshot again and compare:

   ```bash
   .claude/skills/slop/scripts/snapshot.sh /tmp/slop-after
   diff -r /tmp/slop-before /tmp/slop-after
   ```

   Cleanup of generator logic leaves the output identical, so a difference
   there means an edit changed behavior; revert it. Cleanup of emitted code,
   the `quote!` templates in `codegen/`, changes the output on purpose. Confirm
   every difference comes from those edits, then rebuild the fixtures:

   ```bash
   .claude/skills/testing/scripts/build-fixtures.sh
   ```

   Delete `/tmp/slop-before` and `/tmp/slop-after` afterward.

2. Run the project checks. The snapshot covers only `fixtures/petstore.json`;
   `cargo test` covers the rest. `--no-fail-fast` keeps a failing test binary
   from hiding the ones after it.

   ```bash
   cargo +nightly fmt --all
   cargo clippy --all -- -W clippy::pedantic
   cargo test --no-fail-fast
   ```

Finish with one line per file listing what was removed. Quote any failing
command output.
