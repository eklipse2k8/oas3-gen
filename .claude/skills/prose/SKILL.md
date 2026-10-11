---
name: prose
description: This skill should be used when writing or editing oas3-gen prose outside the book, including when the user asks to "update the README", "write a design doc", or "edit docs/ or CLAUDE.md", and when changing the text of `README.md`, `CONTRIBUTING.md`, `CLAUDE.md`, or `docs/`. Provides the project's writing rules and per-artifact conventions.
---

# Writing oas3-gen Prose

## Writing Rules

- **State facts.** Say what a thing is and what it does. Give every sentence
  information the reader needs.
- **Use present tense.** Describe how the system works now.
- **Document current behavior only.** In every artifact, commit messages and PR
  descriptions included, leave out how the design evolved, rejected
  alternatives, deprecated approaches, and planned work. The diff and
  `git blame` record what changed. A reason that still holds is current and
  stays.
- **Use the literal word.** Replace metaphor and praise with the behavior behind
  it: write “by default,” not “out of the box”; say what the code does instead
  of “magic” or “under the hood”; describe what would earn “seamless,”
  “robust,” or “ergonomic.” A metaphor that no literal word can replace marks an
  idea that needs more thought.
- **Cut filler.** Drop buzzwords such as “leverage,” “synergy,” “paradigm,” and
  “stakeholders”; intensifiers and hedges such as “very,” “quite,” and
  “somewhat”; and alarm words such as “critical” and “essential” unless
  something breaks without the thing described.
- **Use active voice.** Write “`SchemaConverter` turns each schema into
  `RustType` values,” not “Each schema is turned into `RustType` values by
  `SchemaConverter`.”
- **Show concrete examples.** Prefer a real identifier, command, spec fragment,
  or generated type to a description of one.

## Agent Docs

In `CLAUDE.md` and `docs/`, record only what the code can't state: rules and
the reasons they hold, gotchas, and where a change belongs. Point to the source
for everything else, because copies go stale. List each new doc in the
`CLAUDE.md` docs table with the situation that calls for it.

Order a design doc this way:

1. What the thing is.
2. Why it exists: the problem it solves.
3. What it does.
4. How to use it, when that applies.
5. Examples.

