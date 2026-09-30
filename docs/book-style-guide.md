# Book Style Guide

Use this guide when writing or editing `book/src/`. The book should help a Rust
programmer understand what `oas3-gen` generates, choose its options, and use the
result. Assume familiarity with Rust structs, enums, Cargo, and `Result`, but
explain concepts specific to OpenAPI and this generator when you introduce them.

## The Reference Voice

These observations come from representative chapters of
[*The Rust Programming Language*](https://doc.rust-lang.org/book/). The rules
below adapt that teaching approach to this project's shorter user guide.

The voice is that of a knowledgeable, patient colleague. The
[introduction](https://doc.rust-lang.org/book/ch00-00-introduction.html) tells
readers what knowledge they need and how the chapters fit together. It addresses
the reader directly and allows different routes through the material. Authority
comes from explaining the subject, without requiring a formal or impersonal tone.

[Hello, Cargo!](https://doc.rust-lang.org/book/ch01-03-hello-cargo.html) teaches
through a sequence of actions and observations. It introduces a tool's purpose,
gives a command, then explains the files or output that command produces.
Conversational transitions connect each step to what the reader has just learned.

[What Is Ownership?](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html)
acknowledges that an unfamiliar concept takes practice. It defines terms before
depending on them and develops an example in stages. Encouragement supports the
explanation; it doesn't replace it or suggest that difficulty is the reader's fault.

[Recoverable Errors with Result](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html)
shows an explicit implementation before introducing a shorter form. It explains
what each branch does and why the result matters to the caller. Failures are
part of the lesson, with enough context to understand and resolve them.

## Voice and Wording

1. **Address the reader as “you.”** Use “we” for an example we're working through
   together. Name the generator when describing its behavior. Avoid switching to
   “the developer” or “the consumer” when you mean the reader.
2. **Sound conversational and precise.** Use natural contractions such as “you'll”
   and “doesn't.” Prefer “use,” “create,” and “return” to abstract descriptions.
   Use “let's” when beginning a shared example, without repeating it mechanically.
3. **Explain benefits through behavior.** Replace praise such as “ergonomic,”
   “comprehensive,” or “builders shine” with the action a reader can take or the
   work an option saves. Don't promise performance or compatibility without evidence.
4. **Respect the learning process.** Avoid “obviously,” “just,” and “simply” when
   they dismiss a task's difficulty. Explain a confusing result and the next step.
5. **Write connected explanations.** Give each paragraph one main point. Follow
   a claim with its reason or consequence. Avoid slogan-like fragments, repeated
   bold labels, and several unrelated details packed into one sentence.

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

## Structure and Formatting

11. **Use descriptive headings.** Keep title case and the existing chapter
    hierarchy. Prefer subjects or tasks over promotional headings. Preserve
    published anchors when possible, and update links when headings change.
12. **Choose the format that teaches the point.** Use paragraphs for explanations,
    numbered lists for sequences, bullets for distinct rules, and tables for
    mappings or option comparisons. Keep the flag summary as a reference.
13. **Keep presentation quiet.** Use code formatting for identifiers, flags, paths,
    and literal values. Label fenced blocks with their language. Let headings
    separate sections; avoid decorative rules and unnecessary callouts. Follow
    the repository's prohibition on emojis and inline code comments; put
    explanations outside code blocks and retain relevant Rustdoc.

## Accuracy and Review

14. **Ground explanations in the project.** Check generator behavior in source and
    fixtures. Preserve conditions and exceptions while simplifying wording.
    Distinguish schema builders from request builders and compile-time guarantees
    from runtime checks. A prose edit must not imply a new feature.

For example, replace “Three lines. No nested structs. Required fields are
enforced at compile time” with “The request builder assembles the nested structs
for you. You must set each required parameter before calling `build()`. That
call returns a `Result` because the constructor also validates the request.”

Before finishing, check that the reader can identify the purpose, follow the
example, and explain its result. Build the book with `mdbook build book`, check
local links and changed examples, and report any validation you couldn't run.
