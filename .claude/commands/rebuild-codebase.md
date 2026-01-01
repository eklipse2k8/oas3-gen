---
description: Refactor entire chunks of the codebase.
---

# Role
You are the **Lead System Architect** and **Refactoring Orchestrator**. You are managing a team of sub-agents (parallel threads) to completely rebuild a legacy codebase.

## Input
- **TARGET**: $ARGUMENTS (directory path or comma-separated file list)

---

# The Mission
Your goal is to ingest a set of source files, construct a **Directed Acyclic Graph (DAG)** of all logic, reorganize the code into clean "Areas of Concern" (Domains) based on that graph, and rewrite the code to be strictly optimized and simplified.

---

# Process Architecture
Execute this in 4 distinct phases.

## Phase 1: Distributed Symbol Extraction (Parallel)

**Objective:** Build a complete inventory of all code symbols and their relationships.

**Execution:**
1. Use the Task tool with `subagent_type=Explore` to spawn parallel agents, each assigned to different source files or directories
2. Each agent extracts for every function, struct, enum, trait, and module:
   - **Node:** Name, type (fn/struct/enum/trait/mod), visibility (pub/private), file location
   - **Input Edges:** Parameters, generic bounds, trait requirements, imported dependencies
   - **Output Edges:** Return types, side effects (mutations, I/O), exported items

**Output Format:** Each agent returns a JSON structure:
```json
{
  "file": "path/to/file.rs",
  "nodes": [
    {
      "name": "function_name",
      "kind": "fn",
      "visibility": "pub",
      "line": 42,
      "inputs": ["&Schema", "&Config"],
      "outputs": ["Result<TypeRef, Error>"],
      "calls": ["helper_fn", "OtherStruct::method"],
      "called_by": []
    }
  ]
}
```

**Parallelization Strategy:**
- Group files by directory (e.g., `converter/`, `codegen/`, `analyzer/`)
- Launch one Task agent per directory with 3-5 files
- Use `run_in_background: true` for all agents, then collect with TaskOutput

---

## Phase 2: DAG Synthesis & Clustering (Sequential - YOU DO THIS)

**Objective:** Build the dependency graph and identify logical domains.

**Tasks:**
1. **Aggregate Results:** Collect all JSON outputs from Phase 1 agents
2. **Build Dependency Graph:**
   - Create adjacency list: `node -> [nodes it depends on]`
   - Create reverse adjacency: `node -> [nodes that depend on it]`
3. **Topological Sort:** Order nodes so dependencies come before dependents
4. **Cycle Detection:** Flag any circular dependencies (these must be broken)
5. **Cluster Analysis:**
   - Calculate cohesion: nodes that frequently call each other belong together
   - Calculate coupling: minimize dependencies between clusters
   - Use metrics: internal edges / total edges for each potential cluster

**Clustering Algorithm:**
```
1. Start with each file as its own cluster
2. Compute edge density between all cluster pairs
3. Merge clusters with highest density (>0.3 edge ratio)
4. Repeat until no more merges improve modularity
5. Name clusters by their dominant responsibility
```

**Deliverable:** A "Refactoring Plan" document:
```markdown
## Proposed Architecture

### Domain: [DomainName]
**Responsibility:** [One sentence description]
**Files to create:** [new_module.rs]
**Original sources:**
- old_file1.rs: functions [a, b, c]
- old_file2.rs: functions [d, e]

### Domain: [AnotherDomain]
...

## Dependency Order
1. utilities (no dependencies)
2. core_types (depends on: utilities)
3. converters (depends on: core_types, utilities)
...

## Circular Dependencies to Break
- A -> B -> C -> A: Recommend extracting shared trait
```

**Present this plan to the user for approval before proceeding.**

---

## Phase 3: Optimized Reconstruction (Parallel)

**Objective:** Rewrite each domain from scratch with clean architecture.

**Execution:**
1. For each approved domain/cluster, spawn a Task agent with:
   - List of original functions to incorporate
   - The domain's responsibility statement
   - Dependency constraints (what it can import)
   - Target file path

2. Each agent must:
   - **Analyze:** Read all original functions assigned to this domain
   - **Simplify:** Apply Ockham's Razor - find the simplest correct implementation
   - **Combine:** Merge redundant logic, eliminate dead code
   - **Type:** Enforce strict typing, use newtypes where appropriate
   - **Structure:** Apply proper Rust idioms (see guidelines below)

**Rust Refactoring Guidelines for Agents:**
- Convert parameter threading to state-carrying structs
- Use `From`/`TryFrom`/`FromStr` for conversions
- Replace boolean flags with enums
- Use `Option<T>` instead of sentinel values
- Prefer iterators over manual loops
- No explicit lifetimes unless absolutely necessary (use `Rc`/`Arc` instead)
- Keep all types at file top level (no nested `mod {}` blocks)

**Output:** Complete, formatted Rust source files that:
- Compile without errors
- Have no clippy warnings
- Follow the project's naming conventions

---

## Phase 4: Integration & Verification (Sequential - YOU DO THIS)

**Objective:** Ensure all pieces fit together correctly.

**Tasks:**
1. **Collect Outputs:** Gather all rewritten modules from Phase 3
2. **Verify Exports:** Each module exports its public interface correctly
3. **Update Imports:** Ensure cross-module imports use new paths
4. **Wire Entry Points:** Update `mod.rs` / `lib.rs` to expose the new structure
5. **Build Verification:**
   ```bash
   cargo check 2>&1
   cargo clippy 2>&1
   cargo test 2>&1
   ```
6. **Fix Issues:** If compilation fails, identify and fix the issues
7. **Final Report:** Summarize what was changed and the new architecture

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
