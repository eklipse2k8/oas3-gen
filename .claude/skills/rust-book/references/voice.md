# Voice

The book's voice is adapted from *The Rust Programming Language*. This file has
three sections:

- The Reference Voice: the Rust Book chapters the voice is modeled on.
- Voice and Wording: rules 1 to 5 in full.
- Rewrites from This Book: before-and-after pairs that apply the rules.

## The Reference Voice

These observations come from representative chapters of
[*The Rust Programming Language*](https://doc.rust-lang.org/book/). The rules
adapt that teaching approach to this project's shorter user guide.

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

## Rewrites from This Book

The first pair is the original style guide's example. The others come from
`builders.md` and `introduction.md` in commit `1cb0bff`, which adopted this
style. Each heading names the rules the rewrite applies; rules 6 to 13 are in
the other reference files, and rule 14 is in SKILL.md.

### Slogans Become an Explanation (Rules 3, 5, and 14)

Before:

> Three lines. No nested structs. Required fields are enforced at compile time.

After:

> The request builder assembles the nested structs for you. You must set each
> required parameter before calling `build()`. That call returns a `Result`
> because the constructor also validates the request.

The rewrite replaces fragments with connected sentences and separates the
compile-time requirement from the runtime validation.

### The Developer Becomes the Reader (Rules 1 and 10)

Before:

> The developer must remember to call `.validate()` separately. Forgetting it
> means constraints like minimum string length go unchecked at runtime.

After:

> The call to `validate()` checks the constraints after construction. In this
> example, both strings must be nonempty. Constructing the struct alone doesn't
> perform that check.

### Dismissive Wording Becomes an Observation (Rules 4 and 7)

Before:

> The builder treats `Option` fields as optional setters: calling `.tag(...)`
> wraps the value in `Some` automatically, and omitting the call leaves it as
> `None`. The same applies to `allergies`, which can simply be left off when not
> needed.

After:

> Notice that `tag()` accepts a `String`. The builder wraps it in `Some` for us.
> If we leave out that call, `tag` is `None`, as in our first example. The
> `allergies` field is also `None` because we haven't set it.

### Praise Becomes a Choice (Rules 3 and 11)

The heading “When Builders Shine” became “Using Builders in Tests.” Its closing
advice changed as well.

Before:

> For projects that want the lightest possible generated output with zero extra
> dependencies, leaving `--enable-builders` off is the right call. For projects
> that prioritize developer ergonomics and are already comfortable with
> proc-macro dependencies, builders pay for themselves quickly in reduced
> boilerplate and fewer construction errors.

After:

> You can still use struct literals when builders are enabled. Choose a builder
> when setting fields by name or constructing nested requests helps your code,
> and use a literal when you want to show the whole value in one place. If you
> don't need builders, leave `--enable-builders` off to avoid the `bon`
> dependency.

### Bold Labels Become Actions (Rules 3, 5, and 13)

Before:

> - **Serde Support**: Full serialization/deserialization with `serde`
> - **Builder Pattern**: Optional `bon` integration for ergonomic struct
>   construction via `--enable-builders`

After:

> The generator supports several parts of working with an OpenAPI description:
>
> - Generate structs, enums, and type aliases from schemas, including unions
> - Call API operations through an asynchronous `reqwest` client.
