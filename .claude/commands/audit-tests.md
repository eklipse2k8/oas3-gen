You are a test quality auditor. Your task is to analyze a test file line-by-line, identify low-quality or misplaced tests, and refactor to keep only high-quality tests that belong in the file.

## Input
- **FILENAME**: $ARGUMENTS

---

## Phase 1: Deep Analysis (Required First Step)

Read the entire test file and for EACH test function, answer these questions:

### 1.1 Purpose Analysis
- **What production code does this test exercise?**
- **What specific behavior or edge case is being verified?**
- If the test only exercises test helper code, mark it as "junk test"

### 1.2 Location Analysis
- **Does this test belong in this file?**
- Check if the code under test lives in a different module
- Tests for `utils/schema_ext.rs` should NOT be in `converter/tests/`
- Tests for `ast/documentation.rs` should NOT be in `converter/tests/`

### 1.3 Over-Engineering Analysis
- **Is the test setup more complex than necessary?**
- **Are there unnecessary intermediate variables?**
- **Could the test be simplified while testing the same thing?**

### 1.4 Assertion Quality
- **Are assertions specific and meaningful?**
- Weak: `assert!(!result.is_empty())` - just checks non-empty
- Strong: `assert_eq!(result, expected_value)` - verifies exact result
- Unfocused tests with multiple unrelated assertions should be split or removed

---

## Phase 2: Classification

Classify each test into one of these categories:

### Keep (High Quality)
- Tests production code in the correct module
- Has clear, specific assertions
- Tests meaningful behavior or edge cases

### Move (Misplaced)
- High-quality test but belongs in a different module
- Tests for `utils/schema_ext.rs` should be in `utils/` tests
- Tests for `ast/documentation.rs` should be in `ast/tests/`
- Move the test to the correct location, do not delete it

### Remove (Junk)
- Only tests helper functions defined in the test file itself
- Has weak/unfocused assertions that don't verify real behavior
- Duplicates coverage from other tests

### Refactor (Improvable)
- Good intent but over-engineered setup
- Could be consolidated with similar tests using table-driven pattern
- Needs better assertion messages

---

## Phase 3: Refactoring Rules

### 3.1 Test Naming (Idiomatic Rust Style)
Follow Rust standard library conventions:
- NO `test_` prefix (the `#[test]` attribute already indicates it's a test)
- Use snake_case describing the behavior
- Good: `parse_valid_input`, `returns_none_for_empty`
- Bad: `test_parse_valid_input`, `test_returns_none_for_empty`

### 3.2 Helper Function Naming
- `string_schema()` not `create_string_schema()` - shorter, still clear
- `num(n)` for simple conversions
- Place at top of file after imports

### 3.3 Table-Driven Consolidation
Merge tests that:
- Test the same function with different inputs
- Have identical setup patterns
- Only differ in the values being tested

```rust
#[test]
fn validation_format_mapping() {
  let cases = [
    ("email", ValidationAttribute::Email, "email format"),
    ("url", ValidationAttribute::Url, "url format"),
    ("uri", ValidationAttribute::Url, "uri maps to Url"),
  ];
  for (format, expected, desc) in cases {
    let mut schema = string_schema();
    schema.format = Some(format.to_string());
    let attrs = extract_validation(&schema);
    assert!(attrs.contains(&expected), "{desc}");
  }
}
```

### 3.4 Assertion Context
Always include context for debugging:
```rust
assert_eq!(result, expected, "failed for {input:?}");
assert!(condition, "{desc}");
```

---

## Phase 4: Output Format

Present findings in this structure:

### 1. Test Inventory
```
File: path/to/tests.rs
Total tests: N

| Test Name | Category | Reason |
|-----------|----------|--------|
| test_foo  | Remove   | Tests helper, not production code |
| test_bar  | Move     | Tests SchemaExt, belongs in utils/ |
| test_baz  | Refactor | Consolidate with test_qux |
| test_qux  | Refactor | Consolidate with test_baz |
| test_good | Keep     | Tests production edge case |
```

### 2. Action Plan
```
Moves:
- test_bar -> utils/tests/schema_ext_tests.rs (tests SchemaExt)

Removals:
- test_foo: Only tests make_foo() helper defined in tests

Consolidations:
- test_baz, test_qux -> baz_and_qux_scenarios (table-driven)

Helpers to extract:
- string_schema() - used 5 times
- num(n) - simplifies json number creation
```

### 3. Write Refactored File
Use the Write tool to output the cleaned-up test file.

### 4. Summary Statistics
```
Before: N tests, M lines
After:  X tests, Y lines
Moved: A tests to correct modules
Removed: B junk tests
Consolidated: C tests -> D tests
Reduction: Z%
```

### 5. Verification
- Run `cargo test <module_name>` to verify all tests pass
- Run `cargo clippy --fix --all --tests --allow-dirty -- -W clippy::pedantic`
- Run `cargo +nightly fmt --all`

---

## Constraints

- NEVER remove tests that verify unique production code behavior
- NEVER remove high-quality tests just because they're in the wrong file - move them instead
- ALWAYS verify removed tests were truly testing non-production code
- ALWAYS run tests after changes to ensure nothing broke
- DO NOT add inline comments to explain tests - tests should be self-documenting
- DO NOT use emojis anywhere
