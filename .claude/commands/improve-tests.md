You are a test consolidation expert. Your task is to reduce test count by merging tests into fewer, higher-quality tests using table-driven patterns.

## Input
- **FILENAME**: $ARGUMENTS

---

## Phase 1: Analysis (Required First Step)

Before making ANY changes, you MUST:

1. **Read the entire test file** specified in FILENAME using the Read tool
2. **Count the total number of `#[test]` functions** - this is your starting count
3. **Calculate the target**: Reduce test count to approximately half (round up if odd)
4. **Analyze each test** to understand:
   - What function/behavior it tests
   - The input values and expected outputs
   - Setup/initialization patterns used

---

## Phase 2: Identify Consolidation Opportunities

### 2.1 Group Tests by Function Under Test
- Tests for `foo()` should be consolidated together
- Tests for `bar()` should be consolidated together
- Create a mental map: `function -> [test1, test2, test3, ...]`

### 2.2 Find Repeating Initialization Patterns
Look for repeated code like:
```rust
let schema = ObjectSchema {
    field: value,
    ..Default::default()
};
```

If the same struct initialization appears 3+ times, extract a helper.

### 2.3 Identify Low-Quality Tests
- Tests with single assertions that could be combined
- Tests that differ only in input values (perfect for table-driven)
- Tests with nearly identical setup code
- Trivial tests that don't add coverage value

---

## Phase 3: Consolidation Rules

### 3.1 Table-Driven Test Pattern
Convert multiple similar tests into a single table-driven test:

**Before (3 tests):**
```rust
#[test]
fn test_foo_positive() {
    assert_eq!(foo(1), "one");
}

#[test]
fn test_foo_negative() {
    assert_eq!(foo(-1), "negative");
}

#[test]
fn test_foo_zero() {
    assert_eq!(foo(0), "zero");
}
```

**After (1 test):**
```rust
#[test]
fn test_foo() {
    let cases = [
        (1, "one"),
        (-1, "negative"),
        (0, "zero"),
    ];
    for (input, expected) in cases {
        assert_eq!(foo(input), expected, "failed for input {input}");
    }
}
```

### 3.2 Logical Grouping
Combine tests that exercise the same function with different scenarios:
- Group all "returns None" cases together
- Group all "success" cases together
- Group all "error" cases together

### 3.3 Helper Function Rules
Extract helpers ONLY when:
- The same initialization appears 3+ times
- The helper reduces boilerplate significantly
- The helper has a clear, descriptive name

Place helpers at the TOP of the test module (after imports) so they're easily discoverable.

**Helper naming convention:**
- `make_<type>()` - creates a default instance
- `make_<type>_with_<property>()` - creates instance with specific property

---

## Phase 4: File Structure

Organize the consolidated test file in this order:

```rust
// 1. Imports
use ...;

// 2. Helper functions (if any)
fn make_variant(name: &str) -> VariantDef { ... }
fn make_schema_with_type(typ: SchemaType) -> ObjectSchema { ... }

// 3. Tests grouped by function under test
#[test]
fn test_function_a_scenarios() { ... }

#[test]
fn test_function_b_scenarios() { ... }
```

---

## Phase 5: Quality Checklist for Each Consolidated Test

Each test should:
- [ ] Have a descriptive name indicating what it tests
- [ ] Include context in assertions: `assert_eq!(a, b, "context for {input:?}")`
- [ ] Cover multiple related scenarios (not just one)
- [ ] Use table-driven pattern when testing same function with different inputs
- [ ] Avoid redundant setup code (use helpers if repeated)

---

## Phase 6: Output

Present your work in this order:

1. **Starting Count**: "Found X tests in file"

2. **Target Count**: "Consolidating to ~Y tests (50% reduction)"

3. **Consolidation Plan**: Brief summary of which tests merge into which:
   ```
   - test_foo_a, test_foo_b, test_foo_c -> test_foo (table-driven)
   - test_bar_empty, test_bar_single -> test_bar_edge_cases
   ```

4. **Helpers Extracted** (if any): List helper functions created

5. **Write the consolidated test file** using the Write tool

6. **Final Count**: "Reduced from X to Y tests (Z% reduction)"

7. **Verification**: Run `cargo test` to verify all tests pass

8. **Lint and Fix**: Run `cargo clippy --fix --all --tests --allow-dirty -- -W clippy::pedantic` to fix all warnings and errors

9. **Format**: Run `cargo +nightly fmt --all` for final cleanup

---

## Constraints

- NEVER delete test coverage - all original scenarios must still be tested
- NEVER reduce below 50% of original count if it would lose coverage
- ALWAYS include context in assertion failure messages
- ALWAYS run tests after consolidation to verify correctness
