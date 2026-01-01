---
description: Refactor entire chunks of the codebase.
---

# Role

You are the **Lead System Architect** and **Compiler Engineer**. You are managing a team of sub-agents to perform a "clean-slate" rebuild of a legacy codebase by analyzing it down to the statement level.

## Input

- **TARGET**: $ARGUMENTS (directory path or comma-separated file list)

---

# Process Architecture

## Phase 1: Granular Symbol & Logic Extraction (Parallel)

**Objective:** Build a complete "Knowledge Graph" of the code, capturing not just function signatures but the internal logic flow of every block.

**Execution:**

1. Spawn agents using `subagent_type=Explore`.
2. **Deep Analysis:** For every file, do not just read the text. Parse the **Abstract Syntax Tree (AST)**.
3. **Extract:**
    - **Symbols:** Functions, Structs, Enums, Traits.
    - **Internal Dependencies (The "Every Line" Requirement):** Inside each function, list every external symbol it touches (e.g., `Function A` calls `Function B`, uses `Struct C`, and accesses `Global D`).
    - **Logic Fingerprint:** A pseudo-code summary of the *control flow* (e.g., "Loop -> Match -> Recursive Call").

**Output Format (Strict JSON):**

```json
{
  "file": "src/processor.rs",
  "symbols": [
    {
      "id": "processor::process_data",
      "type": "fn",
      "lines": "40-85",
      "incoming_edges": [],
      "outgoing_edges": [
         "config::Config::new",  // Tracks dependency on other struct
         "std::fs::write"        // Tracks I/O side effect
      ],
      "internal_logic": "Matches on Config enum; if Valid, maps data; else returns Error."
    }
  ]
}

```

---

## Phase 2: DAG Synthesis & Architecture Plan (Sequential - YOU DO THIS)

**Objective:** specific logical organization based on the data flow.

**Tasks:**

1. **Ingest:** Load all JSON outputs.
2. **Construct the Macro-DAG:** Link nodes based on `outgoing_edges`.
3. **Cluster:** Group symbols that share high connectivity (Logic Units) rather than just file proximity.
4. **Visualize:** Generate a **Mermaid.js** chart representing this new architecture.

**Deliverable:** A "Refactoring Blueprint" containing:

1. **The Proposed Dependency Graph (Mermaid):**

```mermaid
graph TD
  subgraph "Domain: Ingestion"
    A[RawParser] --> B[Validator]
  end
  subgraph "Domain: Core"
    B --> C[ItemStruct]
  end

```

1. **The Refactoring Plan:**

* **Domain:** [Name]
- **Files:** [List of new files]
- **Rationale:** "Merged `old_a.rs` and `old_b.rs` because they share 80% of their data structures."

**Action:** Pause here. Show the Mermaid graph to the user. Wait for approval.

---

## Phase 3: Statement-Level Reconstruction (Parallel)

**Objective:** Rebuild the logic from the ground up using the graph.

**Execution:**

1. Assign one agent per new **Domain Cluster**.
2. **The "Complete Tree" Logic Rebuild:**

* Instead of copying code, read the "Logic Fingerprint" and `internal_logic` from Phase 1.
- **Build a Micro-DAG:** For the specific functions in this domain, map the flow of data variables statement-by-statement.
- **Optimize:** Look at this Micro-DAG. If you see `A -> B -> C` but `A` could go straight to `C`, rewrite it.
- **De-duplicate:** If two branches of the tree look identical, extract a helper function.

**Refactoring Rules:**

- **No "Blind Copying":** Do not paste old implementation details. Re-implement the *intent* of the logic using modern Rust patterns.
- **Strict Typing:** Replace all "stringly typed" logic with Enums/Structs.
- **Error Handling:** Propagate errors using `?`; no `.unwrap()`.

**Output:**

- The new source file (e.g., `domain_ingestion.rs`).
- A brief report: "Reduced 200 lines of spaghetti code to 50 lines of clean iterators."

---

## Phase 4: Integration (Sequential)

**Objective:** Wire the optimized domains together.

1. Generate `mod.rs` / `lib.rs` that exposes the new clean architecture.
2. Run `cargo check`.
3. If compilation fails (usually due to imports), trace the DAG to find where the dependency broke and fix it.

---

# Execution Constraints

## Strict DAG Adherence
- No circular dependencies between modules
- Code flow must be unidirectional where possible
- If cycles exist in original code, they must be broken by extracting shared abstractions

## Type Safety
- All data flowing between modules must have matching types
- Use newtypes to distinguish semantically different values of the same underlying type
- Prefer `Result<T, E>` over panics

## Naming Conventions
- Modules: `snake_case`
- Types: `PascalCase`
- Functions: `snake_case`, verb-first (`parse_`, `convert_`, `emit_`)
- Constants: `SCREAMING_SNAKE_CASE`

## Testing
- Preserve existing test coverage
- Update test imports to match new structure
- Add integration tests for cross-module interactions

---

# Immediate Next Step

1. Parse the TARGET argument to identify source files
2. Validate the paths exist
3. Begin **Phase 1** by spawning parallel extraction agents
4. Report progress using TodoWrite tool

If TARGET is empty or unclear, ask the user to specify the directory or files to analyze.
