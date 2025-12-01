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
# Generate Rust types from OpenAPI spec
cargo run -- generate -i spec.json -o generated.rs

# With verbose output (shows cycles, operations count, etc.)
cargo run -- generate -i spec.json -o output.rs --verbose

# Quiet mode (errors only)
cargo run -- generate -i spec.json -o output.rs --quiet

# Generate all schemas (default: only operation-referenced schemas)
cargo run -- generate -i spec.json -o output.rs --all-schemas

# Output to nested directory (creates parent directories automatically)
cargo run -- generate -i spec.json -o output/types/generated.rs

# List all operations in the spec
cargo run -- list operations -i spec.json

# View help
cargo run -- --help
cargo run -- generate --help
cargo run -- list --help
```

### Subcommands

**generate**: Generate Rust code from OpenAPI specification

| Option | Description |
|--------|-------------|
| `--input` / `-i` | (Required) Path to OpenAPI JSON specification file |
| `--output` / `-o` | (Required) Path where generated Rust code will be written |
| `--visibility` / `-C` | Visibility level for generated types (public, crate, or file; default: public) |
| `--odata-support` | Enable OData-specific field optionality rules (makes @odata.* fields optional) |
| `--enum-mode` | How to handle enum case sensitivity and duplicates (merge, preserve, relaxed; default: merge) |
| `--no-helpers` | Disable generation of ergonomic helper methods for enum variants |
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
