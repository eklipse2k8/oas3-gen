You are a Rust code generation architecture expert. Your task is to refactor code generation modules into small, composable "Fragment" types that each implement `ToTokens` for emitting Rust code.

## Input
- **TARGET**: $ARGUMENTS

---

## Phase 1: Analysis

Before making changes:

1. **Read the target file(s)** specified in TARGET
2. **Identify the AST types** being consumed (from the `ast/` module)
3. **Trace the code generation flow** to understand how generators produce `TokenStream` output
4. **Map the existing structure**:
   - Which generators exist?
   - What code do they emit?
   - Which parts are repeated or could be shared?

---

## Phase 2: Decomposition Strategy

Break apart large generators into composable Fragment types following these principles:

### 2.1 Fragment Design Rules

**Ownership**: Fragments MUST own their data (no lifetime parameters)
- Clone data from AST types into Fragment fields
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
  // Owned data extracted from AST
  name: SomeToken,
  field: String,
  // Pre-computed TokenStreams for complex attributes
  attrs: TokenStream,
}

impl SomeFragment {
  pub(crate) fn new(ast_type: AstType) -> Self {
    // Extract and transform data from AST
    Self {
      name: ast_type.name,
      field: ast_type.field,
      attrs: compute_attrs(&ast_type),
    }
  }
}

impl ToTokens for SomeFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let field = &self.field;
    let attrs = &self.attrs;
    
    let ts = quote! {
      #attrs
      struct #name {
        #field
      }
    };
    
    tokens.extend(ts);
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
   - Replace inline `quote!` blocks with Fragment composition
   - Keep Generators as orchestrators that assemble Fragments

4. **Remove dead code**: Delete replaced helper functions and inline code

---

## Phase 5: Verification

After refactoring:

1. **Run tests**: `cargo test` - all existing tests must pass
2. **Run clippy**: `cargo clippy --all -- -W clippy::pedantic` - no warnings
3. **Rebuild fixtures**: Regenerate any fixture files to verify output unchanged
4. **Check for dead code**: Ensure no unused types or functions remain

---

## Output Format

Present your work as:

1. **Analysis**: Summary of existing structure and identified decomposition points

2. **Fragment Inventory**: List of new Fragment types to create with their responsibilities

3. **Implementation**: The refactored code with all new Fragment types

4. **Verification**: Results of test runs and clippy checks

---

## Example Decomposition

Before (monolithic):
```rust
impl EnumGenerator {
  fn generate(&self) -> TokenStream {
    let variants = self.def.variants.iter().map(|v| {
      let name = &v.name;
      let attrs = generate_attrs(&v.attrs);
      quote! { #attrs #name }
    });
    
    quote! {
      enum #name { #(#variants),* }
    }
  }
}
```

After (composable):
```rust
#[derive(Clone, Debug)]
pub(crate) struct EnumVariantFragment {
  name: EnumVariantToken,
  attrs: TokenStream,
}

impl EnumVariantFragment {
  pub(crate) fn new(variant: VariantDef) -> Self {
    Self {
      name: variant.name,
      attrs: generate_attrs(&variant.attrs),
    }
  }
}

impl ToTokens for EnumVariantFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.name;
    let attrs = &self.attrs;
    tokens.extend(quote! { #attrs #name });
  }
}

impl EnumGenerator {
  fn generate(&self) -> TokenStream {
    let variants: Vec<_> = self.def.variants.iter()
      .map(|v| EnumVariantFragment::new(v.clone()))
      .collect();
    let variants = EnumVariants::new(variants);
    
    quote! {
      enum #name { #variants }
    }
  }
}
```

---

## Checklist

- [ ] All Fragment types own their data (no lifetimes)
- [ ] All Fragment types implement `ToTokens`
- [ ] Fragment names use the `Fragment` suffix
- [ ] Generic containers used where patterns repeat
- [ ] Existing tests pass
- [ ] No clippy warnings
- [ ] No dead code remains
