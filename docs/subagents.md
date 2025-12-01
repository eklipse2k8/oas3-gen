# Available Subagents

This project has specialized subagents that can be invoked using the Task tool for specific types of work.

## performance-engineer

**Purpose**: Optimize CLI performance, reduce memory usage, and improve build times

**When to use**:

- Profiling performance bottlenecks with flamegraphs
- Optimizing schema conversion and code generation speed
- Reducing binary size and startup time
- Implementing benchmarks with criterion

## code-reviewer

**Purpose**: Review code for Rust idioms, safety, and project standards

**When to use**:

- Reviewing code changes for correctness and performance
- Checking adherence to token conservation requirements
- Evaluating AST manipulation and type safety
- Ensuring generated code quality

## test-automator

**Purpose**: Create comprehensive test suites and CI/CD pipelines

**When to use**:

- Writing unit tests for converters and generators
- Setting up property-based testing with proptest
- Creating GitHub Actions workflows
- Testing generated code compilation

## cli-developer

**Purpose**: Enhance CLI interface and user experience

**When to use**:

- Adding new CLI arguments or features
- Improving error messages and progress reporting
- Setting up binary distribution and releases
- Implementing shell completions

## documentation-expert

**Purpose**: Create user-friendly documentation for all audiences

**When to use**:

- Writing installation guides for non-Rust users
- Creating usage examples and troubleshooting guides
- Documenting generated code patterns
- Maintaining README and CHANGELOG

## Subagent Collaboration

These subagents are designed to work together:

- **performance-engineer** -> **code-reviewer** for optimization validation
- **code-reviewer** -> **test-automator** for test coverage requirements
- **cli-developer** -> **documentation-expert** for usage documentation
- **test-automator** -> **performance-engineer** for benchmark creation
