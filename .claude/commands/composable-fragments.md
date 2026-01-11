You are a Rust code generation architecture expert. Your task is to refactor code generation modules into small, composable "Fragment" types that each implement `ToTokens` for emitting Rust code.

## Input
- **TARGET**: $ARGUMENTS

---

## Prerequisites

Before making any changes, read and understand:
1. **CLAUDE.md** - Project guidelines and architecture overview
2. **docs/architecture.md** - Directory structure and module organization
3. **docs/coding-standards.md** - Naming conventions, patterns, and style rules
4. **docs/testing.md** - Fixtures
5. **docs/code-fragments.md** - Complete reference of existing codegen fragments

All changes must adhere to these project standards.

---

## Phase 1: Analysis

Before making changes:

1. **Read the target file(s)** specified in TARGET
2. **Read docs/code-fragments.md** to understand ALL existing fragments
3. **Identify the AST types** being consumed (from the `ast/` module)
4. **Trace the code generation flow** to understand how generators produce `TokenStream` output
5. **Map the existing structure**:
   - Which generators exist?
   - What code do they emit?
   - Which existing fragments can be reused?
   - Which parts are repeated or could be shared?

**CRITICAL: Fragment Reuse Rule**
You MUST reuse existing fragments from `docs/code-fragments.md` whenever possible. Only create a new fragment when:
- No existing fragment handles the required code generation pattern
- The existing fragment cannot be composed or parameterized to achieve the goal
- The pattern is fundamentally different from all existing fragments

Before creating any new fragment, explicitly state:
1. Which existing fragments you considered
2. Why none of them can be used or composed to achieve the goal

---

## Phase 2: Decomposition Strategy

**First, identify which existing fragments can be reused** from `docs/code-fragments.md`. Then break apart generators into their smallest composable Fragment types following these principles:

### 2.1 Fragment Design Rules

**Ownership**: Fragments MUST own their data (no lifetime parameters)
- Store AST types directly when possible (avoid expanding into individual fields)
- Only expand parameters when the AST type cannot be stored directly
- Fragments are immutable after construction

**Single Responsibility**: Each Fragment renders ONE logical piece of code
- A variant definition
- A match arm
- An impl block
- A trait implementation

**Composability**: Small Fragments compose into larger ones
- `VariantFragment` -> `EnumVariants<T>` -> `EnumDefinitionFragment`
- `MatchArmFragment` -> `MatchBlockFragment` -> `ImplFragment`

**Generic Reuse**: Use generics where patterns repeat
```rust
// Good: Generic container for any variant type
pub struct EnumVariants<T: ToTokens>(Vec<T>);

// Good: Reusable across different enum kinds
impl<T: ToTokens> ToTokens for EnumVariants<T> { ... }
```

### 2.2 Naming Convention

Use the `Fragment` suffix for all code generation types:
- `EnumMethodFragment` - renders a single method
- `DisplayImplFragment` - renders a Display impl
- `SerializeImplFragment` - renders a Serialize impl
- `VariantFragment` - renders an enum variant

Reserve `Node` suffix for AST types (input data structures).

### 2.3 Fragment Structure Pattern

Each Fragment should follow this structure:

```rust
#[derive(Clone, Debug)]
pub(crate) struct SomeFragment {
  // Store AST types directly when possible
  def: SomeAstDef,
  // Additional context not in the AST
  visibility: Visibility,
}

impl SomeFragment {
  pub(crate) fn new(def: SomeAstDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for SomeFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let vis = &self.visibility;
    
    tokens.extend(quote! {
      #docs
      #vis struct #name { }
    });
  }
}
```

---

## Phase 3: Common Fragment Categories

Identify and create Fragments for these common patterns:

### 3.1 Variant Fragments
- Value enum variants (unit, tuple, struct)
- Discriminated union variants
- Response enum variants

### 3.2 Impl Block Fragments
- Trait implementations (Display, Serialize, Deserialize, Default)
- Method impl blocks
- Const impl blocks

### 3.3 Match Arm Fragments
- Serialization match arms
- Deserialization match arms
- Display format arms

### 3.4 Container Fragments
- Generic `EnumVariants<T>` for variant lists
- Method collections
- Attribute collections

---

## Phase 4: Refactoring Process

1. **Start with leaf types**: Create Fragments for the smallest pieces first
   - Individual variants
   - Single match arms
   - Single methods

2. **Build up composites**: Create Fragments that contain other Fragments
   - Impl blocks containing method Fragments
   - Enum definitions containing variant Fragments

3. **Update generators**: Modify existing Generator types to use Fragments
   - Replace inline "quote!" blocks with Fragment composition
   - Keep Generators as orchestrators that assemble Fragments

4. **Remove dead code**: Delete replaced helper functions and inline code

---

## Phase 5: Verification

**IMPORTANT**: Use the Task tool to launch a `commander-agent` subagent to execute Phase 5 verification. The subagent ensures all steps are completed in order and reports any concerns back to you.

Launch the subagent with the following prompt:

```
You are a verification agent for a Rust code generation refactoring task. Execute each step IN ORDER and report results back.

## Verification Steps (execute sequentially, except where noted)

### Step 1: Run tests
Run `cargo test` and verify all tests pass.
- If tests fail, report the failures and STOP.
- If tests pass, report the count and proceed.

### Step 2: Rebuild fixtures (run in parallel)
Read docs/testing.md to find the "Rebuilding Fixtures" section with the list of fixture rebuild commands.
Run ALL fixture rebuild commands from that file IN PARALLEL.
- This is the single source of truth for which fixtures exist.
- Do not hardcode fixture names; always read from docs/testing.md.

### Step 3: Run clippy with autofix
Run `cargo clippy --fix --all --tests --allow-dirty -- -W clippy::pedantic`
- Report any warnings that could not be auto-fixed.

### Step 4: Format code
Run `cargo +nightly fmt --all`

### Step 5: Verify fixtures unchanged
Run `git diff --stat crates/oas3-gen/fixtures/`
- If there are NO changes, report success.
- If there ARE changes, run `git diff crates/oas3-gen/fixtures/ | head -100` to show sample changes.
- Formatting-only differences (indentation changes) are acceptable if fixtures were not previously formatted.
- Functional changes (different code being generated) are a CONCERN that must be reported.

### Step 6: Check for dead code
Run `cargo build 2>&1 | grep -E "(dead_code|unused)"` and `cargo clippy --all 2>&1 | grep -E "(dead_code|unused)"`
- Report any dead code warnings.

## Final Report

After completing all steps, provide a summary:
1. Test results (pass/fail count)
2. Fixture rebuild status
3. Clippy status (warnings remaining)
4. Formatting status
5. Fixture diff status (unchanged/formatting-only/functional changes)
6. Dead code status
7. **CONCERNS**: List any issues that need attention from the main agent

If any step fails or raises concerns, clearly state what the main agent needs to address.
```

Wait for the subagent to complete and review its report. If concerns are raised, address them before proceeding to Phase 6.

---

## Phase 6: Remove Indirection (MANDATORY)

**DO NOT SKIP THIS PHASE.** Phase 6 must be completed after Phase 5 verification passes. This phase is critical to eliminate unnecessary indirection and ensure clean architecture.

As a final pass, once all changes have been made and tests pass:

1. **Remove wrapper functions**: Delete any functions that simply delegate to Fragment
2. **Update all call sites**: Change call sites to use Fragment directly
3. **Verify again**: Run tests and clippy one more time

---

## Test Organization

**Do NOT write inline tests** in the module file. Keep all tests in the `tests/` folder following the existing project architecture:

- Module: `codegen/type_aliases.rs`
- Tests: `codegen/tests/type_alias_tests.rs`

Update the `tests/mod.rs` to include any new test modules.

---

## Output Format

Present your work as:

1. **Analysis**: Summary of existing structure and identified decomposition points

2. **Existing Fragment Reuse**: List of existing fragments from `docs/code-fragments.md` that will be reused

3. **Fragment Inventory**: List of new Fragment types to create with their responsibilities. For each new fragment, explain why no existing fragment could be used.

4. **Implementation**: The refactored code with all new Fragment types

5. **Verification**: Results of test runs and clippy checks

---

## Example Decomposition

Before (monolithic):
```rust
pub(crate) fn generate_type_alias(
  _context: &Rc<CodeGenerationContext>,
  def: &TypeAliasDef,
  visibility: Visibility,
) -> TokenStream {
  let name = &def.name;
  let docs = &def.docs;
  let vis = visibility.to_tokens();
  let target = coercion::parse_type_string(&def.target.to_rust_type());

  quote! {
    #docs
    #vis type #name = #target;
  }
}
```

After (composable, no indirection):
```rust
#[derive(Clone, Debug)]
pub(crate) struct TypeAliasFragment {
  def: TypeAliasDef,
  visibility: Visibility,
}

impl TypeAliasFragment {
  pub(crate) fn new(def: TypeAliasDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for TypeAliasFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let target = &self.def.target;
    let vis = &self.visibility;

    tokens.extend(quote! {
      #docs
      #vis type #name = #target;
    });
  }
}
```

Call site (direct usage, no wrapper):
```rust
RustType::TypeAlias(def) => TypeAliasFragment::new(def.clone(), self.visibility).into_token_stream(),
```

---

## Checklist

- [ ] Read CLAUDE.md and coding standards before starting
- [ ] Read docs/code-fragments.md to understand existing fragments
- [ ] Existing fragments reused wherever possible
- [ ] Justification provided for each new fragment (why existing fragments cannot be used)
- [ ] All Fragment types own their data (no lifetimes)
- [ ] All Fragment types implement `ToTokens`
- [ ] Fragment names use the `Fragment` suffix
- [ ] AST types stored directly (not expanded into fields) when possible
- [ ] Generic containers used where patterns repeat
- [ ] No inline tests - all tests in tests/ folder
- [ ] No wrapper functions - call sites use Fragment directly
- [ ] Existing tests pass
- [ ] **Phase 6 completed** - all wrapper functions removed, call sites use Fragments directly
- [ ] No clippy warnings
- [ ] No dead code remains
- [ ] docs/code-fragments.md updated with any new fragments created
