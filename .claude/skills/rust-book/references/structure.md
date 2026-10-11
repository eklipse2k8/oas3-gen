# Structure and Links

This file states rules 11 to 13 in full, then covers where content goes inside a
chapter, how to move sections without breaking links, and how to link external
APIs.

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

## Placing Content Within a Chapter

Keep application walkthroughs in the client, server, and builder chapters.
Use the reference chapters for generation options, schemas and fields, enums
and unions, and shared HTTP behavior. Add a feature to the relevant section
instead of appending another top-level section to a walkthrough. Follow the
order of use: configure the client or router, prepare or receive requests,
then handle responses. Keep advanced details near the concept they extend.

## Moving Sections and Renaming Headings

mdBook derives each heading's anchor from its text, so renaming a heading
changes its URL. When moving a section or renaming a heading:

1. Update every link to the old location. Links can appear on any page, so
   search all of `book/src/` for the old anchor.
2. Rewrite phrases that depend on position, such as “the previous example.”
3. Preserve the old anchor in the `code-generation.md` reference index when
   practical. Each entry there keeps retired anchors as empty elements before
   its link, for example `<a id="types"></a><a id="client"></a>`. Add the
   retired anchor to the entry for the section's new home.
4. Run the check script, which reports links and anchors that no longer resolve.

## Linking External APIs

Link external types, traits, and macros to their Rust API documentation when
introducing them in a section. Use docs.rs pages for the crate versions in
`Cargo.lock`, and the standard library documentation for standard types. Put
links in prose or tables alongside examples so code blocks stay copyable. The
check script warns when a docs.rs link's version differs from `Cargo.lock`, so
run it on the whole book after updating dependencies.

The book writes these as reference-style links. Each page collects its
definitions at the end, named `rustdoc-` followed by the crate and item:

```markdown
[`anyhow::Result<()>`][rustdoc-anyhow-result]

[rustdoc-anyhow-result]: https://docs.rs/anyhow/1.0.104/anyhow/type.Result.html
[rustdoc-option]: https://doc.rust-lang.org/std/option/enum.Option.html
```
