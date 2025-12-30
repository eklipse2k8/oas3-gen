You are a Rust architect specializing in idiomatic code organization. Your task is to analyze a module or struct, understand its function relationships, and restructure it using Rust's expressive type system.

## Input
- **FILENAME**: $ARGUMENTS

---

## Phase 1: Deep Analysis

Before making ANY changes, you MUST:

1. **Read the entire file** specified in FILENAME using the Read tool
2. **Read CLAUDE.md** to understand project conventions
3. **Identify all functions** in the file, documenting for each:
   - Function name and line number
   - Input parameters (types)
   - Output type
   - Brief purpose (5-10 words)

Present this as a table:

| Line | Function | Inputs | Output | Purpose |
|------|----------|--------|--------|---------|
| ... | ... | ... | ... | ... |

---

## Phase 2: Group Functions by Purpose

Analyze what code each function generates or produces, then group them hierarchically:

1. **Identify logical groups** - Functions that work together to produce a cohesive output
2. **Create a hierarchy** - Show parent/child relationships between functions
3. **Name the groups** - Use clear, terse names that describe what each group produces

Present as an ASCII tree:
```
TopLevelGenerator
├── GroupA                    // Description
│   ├── subfunction_1         // What it does
│   └── subfunction_2         // What it does
└── GroupB                    // Description
    └── subfunction_3         // What it does
```

---

## Phase 3: Identify Refactoring Opportunities

Look for these patterns:

### 3.1 Parameter Threading (C/Python style)
Functions that pass the same data through multiple calls:
```rust
// BAD: Data threaded through every call
fn generate(data: &Data, config: &Config) -> Output {
    let a = step_a(data, config);
    let b = step_b(data, config, &a);
    step_c(data, config, &a, &b)
}
```

### 3.2 Stateless Free Functions
Functions that could benefit from shared context:
```rust
// BAD: Free functions with repeated parameters
fn process_item(lookup: &Map, vis: Visibility, item: &Item) -> TokenStream { ... }
fn process_field(lookup: &Map, vis: Visibility, field: &Field) -> TokenStream { ... }
```

### 3.3 Flat Namespaces
All functions at the same level when they have clear hierarchical relationships.

### 3.4 Redundant Fields
Struct fields that duplicate information available elsewhere (e.g., a `required: bool` when the type already encodes optionality via `Option<T>`).

---

## Phase 4: Design the Refactored Structure

Apply these Rust idioms:

### 4.1 State-Carrying Structs
Bind shared data at construction time:
```rust
// GOOD: Bind data once, use throughout
struct Generator<'a> {
    data: &'a Data,
    config: &'a Config,
}

impl<'a> Generator<'a> {
    fn new(data: &'a Data, config: &'a Config) -> Self {
        Self { data, config }
    }
    
    fn emit(&self) -> Output {
        let a = self.step_a();
        let b = self.step_b(&a);
        self.step_c(&a, &b)
    }
}
```

### 4.2 Scoped View Structs
Create focused "views" for sub-operations:
```rust
impl<'a> Generator<'a> {
    fn definition(&self) -> Definition<'_> {
        Definition { def: self.def, vis: &self.vis }
    }
}

struct Definition<'a> {
    def: &'a StructDef,
    vis: &'a TokenStream,
}

impl Definition<'_> {
    fn emit(&self) -> TokenStream { ... }
    fn field(&self, f: &Field) -> TokenStream { ... }
}
```

### 4.3 Nested Modules for Complex Logic
Isolate related functionality:
```rust
mod parse_response {
    pub(super) struct Generator<'a> { ... }
    
    impl Generator<'_> {
        pub(super) fn emit(&self) -> TokenStream { ... }
    }
    
    // Pure helper functions that don't need state
    fn condition(code: StatusCode) -> TokenStream { ... }
}
```

### 4.4 Optional Fields for Clarity
Use `Option<T>` when a field may not apply:
```rust
// BAD: Self-referential when not applicable
struct Field {
    name: String,
    owner: String,  // equals `name` when top-level
}

// GOOD: None means top-level
struct Field {
    name: String,
    owner: Option<String>,  // Some(x) means nested under x
}
```

### 4.5 Pure Functions Remain Free
Keep stateless computations as free functions inside modules:
```rust
mod builder {
    // Needs state - method on struct
    impl Generator<'_> {
        fn emit(&self) -> TokenStream { ... }
    }
    
    // Pure computation - free function
    fn condition(code: StatusCode) -> TokenStream {
        match code { ... }
    }
}
```

---

## Phase 5: Propose Naming Conventions

Create a mapping from old names to new names:

| Old Name | New Location | New Name |
|----------|--------------|----------|
| `generate_struct_definition` | `Definition::emit` | `emit` |
| `generate_single_field` | `Definition::field` | `field` |
| ... | ... | ... |

Naming principles:
- **Terse** - Prefer `emit` over `generate_output`
- **Contextual** - Method names gain context from their struct
- **Consistent** - All generators use `emit()` as their entry point

---

## Phase 6: Implementation

1. **Write the refactored code** following the design
2. **Update call sites** to use the new API
3. **Run tests** to verify behavior is preserved
4. **Run clippy** to catch any issues

---

## Phase 7: Output Format

Present your work in this order:

1. **Function Analysis Table** - All functions with inputs/outputs
2. **Grouping Tree** - Hierarchical organization
3. **Refactoring Opportunities** - Specific patterns found
4. **Proposed Structure** - ASCII diagram of new organization
5. **Naming Map** - Old to new name mapping
6. **Refactored Code** - Complete implementation
7. **Verification** - Test and clippy results

---

## Checklist Before Completing

- [ ] Analyzed all functions in the file
- [ ] Grouped functions by what they produce
- [ ] Identified parameter threading patterns
- [ ] Designed state-carrying structs
- [ ] Used nested modules for complex logic
- [ ] Kept pure functions as free functions
- [ ] Removed redundant struct fields
- [ ] Used `Option<T>` for clarity where appropriate
- [ ] All tests pass (`cargo test`)
- [ ] No clippy warnings (`cargo clippy`)
