---
name: rust-book
description: This skill should be used when writing, editing, reorganizing, or reviewing the oas3-gen mdBook under `book/src/`, including when the user asks to "update the book", "document this feature in the book", "write a book chapter", "add a page to the book", "document a new flag", "edit a chapter", "review the book's prose", "fix the book's voice or tone", "move a section in the book", or "check the book's links", and whenever a feature change must be documented in `book/src/`. Provides the chapter map, a writing workflow, voice and example rules modeled on The Rust Programming Language, and a validation script.
---

# Writing the oas3-gen Book

The book in `book/src/` is an mdBook user guide. It helps a Rust programmer
understand what `oas3-gen` generates, choose its options, and use the result.
Write for readers who know Rust structs, enums, Cargo, and `Result`, and explain
concepts specific to OpenAPI and this generator when introducing them. The voice
follows *The Rust Programming Language*: a knowledgeable, patient colleague who
teaches through actions and observations rather than promotion.

## Workflow

Follow these steps for any change to `book/src/`:

1. **Place the change.** Find the chapter in the chapter map. Add a feature to
   the relevant section of that chapter instead of appending a new top-level
   section.
2. **Confirm the behavior.** Read the generator source and `fixtures/` before
   describing what generated code does. Never describe behavior that hasn't been
   confirmed there.
3. **Draft with the core rules.** Load the reference file for each rule group
   the change touches.
4. **Update connected pages.** Keep `introduction.md` and the `code-generation.md`
   reference index consistent with the chapters. Update `SUMMARY.md` when adding
   or reorganizing chapters.
5. **Validate.** Run the check script with the changed pages as arguments, fix
   every error, and rerun it until it reports none. Review its warnings for
   those pages by hand.
6. **Read it back.** Check that the reader can identify the purpose, follow the
   example, and explain its result. Report any validation that couldn't run.

## Chapter Map

| Page | Kind | Covers |
|---|---|---|
| `client-generation.md` | Walkthrough | Client calls, responses, authentication, and reading Server-Sent Events |
| `server-generation.md` | Walkthrough | Service traits, routing, credentials, and streaming Server-Sent Events |
| `builders.md` | Walkthrough | Construction with builders |
| `generation-options.md` | Reference | Output layouts and command-line options |
| `schema-types.md` | Reference | Fields, collections, defaults, and validation |
| `enum-types.md` | Reference | Enum variants, unions, and discriminators |
| `http.md` | Reference | Shared request, response, and security rules |
| `introduction.md` | Overview | What the generator does and where to start |
| `code-generation.md` | Index | Links to every reference section; keeps retired anchors |

## Core Rules

Each line summarizes a rule. The reference file named in each group states the
rule in full and shows how to apply it.

Voice, in `references/voice.md`:

1. Address the reader as “you,” and use “we” for an example worked together.
2. Sound conversational and precise, with contractions and concrete verbs.
3. Explain benefits through behavior instead of praise.
4. Respect the learning process; don't dismiss difficulty with “just” or “simply.”
5. Write connected explanations, one main point per paragraph.

Examples, in `references/examples.md`:

6. Start with purpose: what the reader will learn and when to use it.
7. Introduce, show, explain: say what an example shows, show it, explain the result.
8. Build one idea at a time, changing only what a comparison needs.
9. Make example boundaries clear: inputs, prerequisites, and omissions.
10. Explain failure at the right level: generation, compilation, or runtime.

Structure, in `references/structure.md`:

11. Use descriptive headings and preserve published anchors.
12. Choose the format that teaches the point.
13. Keep presentation quiet: code formatting, labeled fences, no emojis or
    inline code comments.

Accuracy applies to every change, so it's stated in full here:

14. **Ground explanations in the project.** Check generator behavior in source and
    fixtures. Preserve conditions and exceptions while simplifying wording.
    Distinguish schema builders from request builders and compile-time guarantees
    from runtime checks. A prose edit must not imply a new feature.

## Validation

Run the check script from the repository root:

```bash
python3 -I .claude/skills/rust-book/scripts/check_book.py builders.md http.md
```

The script builds the book into a temporary directory, then reports errors for a
failed build, broken local links or anchors, unlabeled code fences, and emojis.
It exits with status 1 while any error remains. It also warns about dismissive or
promotional words, comments in Rust code blocks, spaces between inline code and
the punctuation after it, and docs.rs links whose version differs from
`Cargo.lock`. Page arguments limit warnings to those pages; errors always cover
the whole book. Run it without arguments after a dependency update to find every
outdated docs.rs link.

A `note:` line means a check didn't run, such as the build and anchor checks
when `mdbook` isn't on `PATH`. Report each note as validation that couldn't run.
The script doesn't verify examples; `references/examples.md` explains how.

## Additional Resources

- **`references/voice.md`**: The Rust Book chapters the voice is modeled on,
  rules 1 to 5 in full, and before-and-after rewrites from this book's history.
  Read it before drafting new prose or revising tone.
- **`references/examples.md`**: Rules 6 to 10 in full, the book's code block
  labels, and how to check examples. Read it before adding or changing commands,
  code, output, or error explanations.
- **`references/structure.md`**: Rules 11 to 13 in full, placement within a
  chapter, moving sections without breaking anchors, and external API links.
  Read it before adding headings, reorganizing, or introducing an external type.
- **`scripts/check_book.py`**: The validator described above. Run it rather than
  reading it.
