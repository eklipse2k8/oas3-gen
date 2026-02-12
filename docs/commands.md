# Build and Development Commands

## Quick Reference

```bash
cargo build                    # Build all crates
cargo test                     # Run all tests
cargo clippy --all -- -W clippy::pedantic  # Lint
cargo +nightly fmt --all       # Format code
```

## Build

```bash
cargo build
```

## Run

The application uses a CLI interface powered by `clap` with subcommands:

```bash
# Generate Rust types from OpenAPI spec (default mode, JSON or YAML auto-detected)
cargo run -- generate types -i spec.json -o generated.rs
cargo run -- generate types -i spec.yaml -o generated.rs

# Generate HTTP client from OpenAPI spec
cargo run -- generate client -i spec.json -o client.rs

# Generate modular client output (types.rs, client.rs, mod.rs in directory)
cargo run -- generate client-mod -i spec.json -o output/

# Generate modular server output (types.rs, server.rs, mod.rs in directory)
cargo run -- generate server-mod -i spec.json -o output/

# With verbose output (shows cycles, operations count, etc.)
cargo run -- generate types -i spec.json -o output.rs --verbose

# Quiet mode (errors only)
cargo run -- generate types -i spec.json -o output.rs --quiet

# Generate all schemas (default: only operation-referenced schemas)
cargo run -- generate types -i spec.json -o output.rs --all-schemas

# Emit all component-level header constants (default: only operation-referenced headers)
cargo run -- generate types -i spec.json -o output.rs --all-headers

# Enable bon builder derives on generated structs
cargo run -- generate client-mod -i spec.json -o output/ --enable-builders

# Custom serde_as type overrides
cargo run -- generate types -i spec.json -o output.rs -c MyDate=my_crate::CustomDate

# Output to nested directory (creates parent directories automatically)
cargo run -- generate types -i spec.json -o output/types/generated.rs

# List all operations in the spec
cargo run -- list operations -i spec.json

# View help
cargo run -- --help
cargo run -- generate --help
cargo run -- list --help
```

### Subcommands

**generate**: Generate Rust code from OpenAPI specification

| Argument/Option | Description |
|-----------------|-------------|
| `[MODE]` | Generation mode: `types` (default), `client`, `client-mod`, or `server-mod` |
| `--input` / `-i` | (Required) Path to OpenAPI specification file (JSON or YAML, auto-detected) |
| `--output` / `-o` | (Required) Path for output (file for types/client, directory for client-mod/server-mod) |
| `--visibility` / `-C` | Visibility level for generated types (public, crate, or file; default: public) |
| `--odata-support` | Enable OData-specific field optionality rules (makes @odata.* fields optional on concrete types) |
| `--enum-mode` | How to handle enum case sensitivity and duplicates (merge, preserve, relaxed; default: merge) |
| `--no-helpers` | Disable generation of ergonomic helper methods for enum variants |
| `--customize` / `-c` | Custom serde_as type overrides (format: type_name=custom::Path); repeatable |
| `--all-headers` | Emit header constants for all header parameters in components, not just those used in operations |
| `--enable-builders` | Enable bon builder derives on schema structs and builder methods on request structs |
| `--only` | Include only the specified comma-separated operation IDs |
| `--exclude` | Exclude the specified comma-separated operation IDs |
| `--all-schemas` | Generate all schemas defined in spec (default: only schemas referenced by operations) |
| `--verbose` / `-v` | Enable verbose output with detailed progress information |
| `--quiet` / `-q` | Suppress non-essential output (errors only) |

**list**: List information from OpenAPI specification

- `operations`: List all operations with their IDs, methods, and paths

**Global Options**:

| Option | Description |
|--------|-------------|
| `--color` | Control color output (always, auto, never; default: auto) |
| `--theme` | Terminal theme (dark, light, auto; default: auto) |

## Linting

### Non-destructive (check only)

```bash
cargo clippy --all -- -W clippy::pedantic

# Check formatting
cargo +nightly fmt --all --check
```

### Destructive (updates files)

```bash
# Automatically fix most warnings, if possible
cargo clippy --fix --allow-dirty --all -- -W clippy::pedantic

# Cleanup any unformatted code
cargo +nightly fmt --all
```

Note: This project uses custom rustfmt settings (see rustfmt.toml):

- 2 spaces for indentation
- 120 character max width
- Merged imports with crate granularity
- Normalized doc attributes

## Dependency Management

```bash
# Check for security advisories and licensing issues
cargo deny check

# Update dependencies
cargo update
```

## Performance Profiling

```bash
# Get available options for flamegraph
cargo flamegraph -h

# Default flamegraph execution of oas3-gen
cargo flamegraph -o flamegraph.svg -- generate -i spec.json -o output.rs
```

## Workspace-Specific Commands

The project uses a Cargo workspace, so you can work with individual crates:

```bash
# Build only the CLI tool
cargo build -p oas3-gen

# Build only the support library
cargo build -p oas3-gen-support

# Run tests for a specific crate
cargo test -p oas3-gen
cargo test -p oas3-gen-support

# Check a specific crate
cargo check -p oas3-gen-support
```

## Book Documentation

The `book/` folder contains user-facing documentation built with mdBook. Feature changes must be documented here.

```bash
# Build the book
mdbook build book/

# Serve locally for preview (http://localhost:3000)
mdbook serve book/

# Clean build artifacts
mdbook clean book/
```

**Files to update for feature changes:**
- `book/src/code-generation.md` - CLI flags and examples
- `book/src/builders.md` - Builder pattern documentation
- `book/src/introduction.md` - Overview and quick start
- `book/src/SUMMARY.md` - Table of contents (if adding new pages)
