# Examples

This file states rules 6 to 10 in full and describes how the book's code blocks
are written and checked.

## Teaching with Examples

6. **Start with purpose.** Open a chapter or section with what the reader will
   learn and when they would use it. For a flag, explain its effect before showing
   its syntax. Keep defaults and restrictions close to the relevant command.
7. **Introduce, show, explain.** Say what an example demonstrates, show the command
   or code, then explain the significant result. Point to concrete names such as
   `Pet::builder()` or `types.rs` instead of saying “the code above is useful.”
8. **Build one idea at a time.** Reuse an example when comparing options. Change
   only what the comparison needs. Explain the ordinary form before a shortcut,
   and connect new concepts to earlier sections with descriptive links.
9. **Make example boundaries clear.** Name input files and prerequisites. Mark
   excerpts that omit imports, derives, or method bodies. Explain where a snippet
   belongs and whether it is intended to compile, fail, or illustrate a structure.
10. **Explain failure at the right level.** Distinguish generation errors,
    compilation errors, and runtime validation errors. State the condition that
    causes each one and what the reader can change. Don't invent diagnostic text.

## Code Blocks in This Book

Label every fence with one of the languages the book already uses:

| Label | Contents |
|---|---|
| `rust` | Generated code and code that calls it |
| `bash` | Generator commands |
| `json` | OpenAPI specification excerpts |
| `toml` | `Cargo.toml` excerpts |
| `text` | Directory trees, flag syntax, and wire formats such as query strings |

Write commands in the form the page already uses. The walkthroughs for clients
and servers and the reference chapters run `cargo run -- generate` from a
checkout of this repository, while `introduction.md` and `builders.md` call an
installed `oas3-gen` binary.

Base examples on the petstore fixture, `fixtures/petstore.json`, when it has the
schema or operation the section needs. Its generated output is checked in as
`fixtures/petstore-client` and `fixtures/petstore-server`, so excerpts can be
compared with real code. When a feature needs a schema the fixture lacks, show
that schema in a `json` block before the generated Rust, as `enum-types.md` does.

The HTML book makes code blocks copyable but not runnable. Readers paste them
into their own projects, so keep explanations, links, and inline comments out of
the block and put them in the surrounding prose.

## Checking Examples

No test compiles the book's Rust blocks, and CI only runs `mdbook build book`.
Check each new or changed example against the checked-in fixtures, or generate
fresh output with `cargo run -- generate` and compare it. Quote output and
diagnostics only after producing them by running the command.
