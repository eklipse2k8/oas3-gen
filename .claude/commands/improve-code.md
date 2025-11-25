You are a code improvement expert. Your task is to deeply analyze and improve code for clarity, readability, and maintainability.

## Input
- **FILENAME**: $ARGUMENTS

---

## Phase 1: Deep Analysis (Required First Step)

Before making ANY changes, you MUST:

1. **Read the entire file** specified in FILENAME using the Read tool
2. **Use extended thinking** to thoroughly understand:
   - The purpose and responsibility of each function
   - Data flow through the code
   - Dependencies between functions
   - Existing patterns and conventions
   - The relationship with tests in the tests/ folder (if any)

3. **Search for related test files** using Glob to find `tests/*.rs` files that cover this code

---

## Phase 2: Identify Improvements

After understanding the code, identify:

### 2.1 Duplicated Logic
- Find repeated code patterns (3+ lines appearing in multiple places)
- Identify similar logic that could be consolidated
- Look for copy-paste code with minor variations

### 2.2 Clarity Issues
- Confusing variable or function names
- Complex conditionals that could be simplified
- Long functions that do too many things
- Magic numbers or strings without context

### 2.3 Simplification Opportunities
- Over-engineered solutions
- Unnecessary abstractions
- Verbose code that could be more concise
- Redundant checks or validations

---

## Phase 3: Refactoring Rules

### CRITICAL: Function Extraction Rule

**NEVER create one-off helper functions.**

When extracting a function:
- The new function MUST be useful to at least 2 call sites (existing or reasonable future use)
- If a piece of logic is only used once, keep it inline
- Prefer keeping related logic together over artificial separation

**Bad Example:**
```rust
fn process_item(item: &Item) -> Result<Output> {
    let validated = validate_item_internal(item)?;  // Only called here
    transform(validated)
}

fn validate_item_internal(item: &Item) -> Result<Item> {  // One-off function
    // validation logic used nowhere else
}
```

**Good Example:**
```rust
fn process_item(item: &Item) -> Result<Output> {
    // Inline validation - only used here
    if item.value < 0 {
        return Err(Error::Invalid);
    }
    transform(item)
}

// OR if validation is used elsewhere:
fn validate_item(item: &Item) -> Result<()> {  // Used by process_item AND import_items
    if item.value < 0 {
        return Err(Error::Invalid);
    }
    Ok(())
}
```

### Other Rules

- Preserve existing public API unless explicitly asked to change it
- Maintain or improve error handling
- Keep changes focused - don't refactor unrelated code
- Follow the project's existing conventions and patterns

---

## Phase 4: Test Coverage

### 4.1 Locate Existing Tests
- Find test files in the `tests/` folder that cover this code
- Understand what is already tested

### 4.2 Ensure High-Quality Tests
After refactoring, ensure tests exist that cover:

- **Happy paths**: Normal successful operations
- **Edge cases**: Empty inputs, boundary values, special characters
- **Error cases**: Invalid inputs, failure conditions
- **Each public function**: At minimum one test per public API

### 4.3 Update or Create Tests
- If tests exist: Update them to match refactored code
- If tests are missing: Create comprehensive tests
- Tests should be in `tests/<module_name>.rs` following project conventions

---

## Phase 5: Output

Present your work in this order:

1. **Analysis Summary**: Brief overview of what you found (duplications, clarity issues, simplification opportunities)

2. **Improvement Plan**: List the specific changes you will make, noting:
   - Which duplicated logic will be consolidated
   - Which functions will be simplified (and how)
   - Any new shared functions being created (with justification for why they serve multiple callers)

3. **Refactored Code**: The complete improved implementation

4. **Test Updates**: Any new or modified tests to ensure coverage

5. **Verification**: Run `cargo test` and `cargo clippy` to verify the changes compile and pass

---

## Checklist Before Completing

- [ ] Read and understood the entire file
- [ ] Identified all duplicated logic
- [ ] No one-off helper functions created
- [ ] All extracted functions serve multiple callers
- [ ] Tests exist and pass for all public functions
- [ ] Code compiles without warnings (`cargo clippy`)
- [ ] All tests pass (`cargo test`)
