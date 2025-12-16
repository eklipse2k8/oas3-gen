# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Improve type safety and attribute handling in code generation ([#42](https://github.com/eklipse2k8/oas3-gen/pull/42))

## [0.22.3] - 2025-12-15

### Added

- Support for parsing and generating webhooks ([#41](https://github.com/eklipse2k8/oas3-gen/pull/41))
- Add remaining HTTP status codes ([#40](https://github.com/eklipse2k8/oas3-gen/pull/40))

## [0.22.2] - 2025-12-09

### Changed

- Changed representation of union object fields in generated fixtures to `HashMap<String, serde_json::Value>` ([#39](https://github.com/eklipse2k8/oas3-gen/pull/39))
- Updated dependencies

## [0.22.1] - 2025-12-07

### Added

- Resolve map-only objects to HashMap types ([#35](https://github.com/eklipse2k8/oas3-gen/pull/35))

## [0.22.0] - 2025-12-03

### Changed

- Enhance type resolution and error handling ([#34](https://github.com/eklipse2k8/oas3-gen/pull/34))
- Only use Bytes type for successful binary responses

## [0.21.1] - 2025-11-27

### Fixed

- Add support for binary response types in response schema extraction

## [0.21.0] - 2025-11-24

### Added

- Use tokens to manage symbols instead of strings ([#31](https://github.com/eklipse2k8/oas3-gen/pull/31))

### Changed

- Update dependencies, refactor validation handling, and improve metadata extraction ([#29](https://github.com/eklipse2k8/oas3-gen/pull/29))
- Refactor converter for improved clarity and error handling ([#30](https://github.com/eklipse2k8/oas3-gen/pull/30))

## [0.20.0] - 2025-11-22

### Added

- Add intelligent enum deduplication and inline type naming ([#26](https://github.com/eklipse2k8/oas3-gen/pull/26))
- Add support for integer path parameters
- Add eventsource support

### Changed

- Update documentation, dependencies, and enhance code structure ([#25](https://github.com/eklipse2k8/oas3-gen/pull/25))
- Refactor cache structures to use BTreeMap and BTreeSet
- Optimize file I/O performance and improve error handling

## [0.17.0] - 2025-11-15

### Changed

- Update OpenAPI generator and enhance error handling ([#24](https://github.com/eklipse2k8/oas3-gen/pull/24))

## [0.16.0] - 2025-11-15

### Added

- OData support with field optionality rules for OData compliance
- OData-specific CLI commands and generation modes
- Stable IDs for generated types
- Generation modes for client or types-only output
- Enhanced header handling in OpenAPI generation

### Changed

- Refined request and response handling in code generation
- Improved CLI argument handling and validation
- Updated dependencies to latest versions

## [0.14.0] - 2025-11-11

### Added

- Dependency graph infrastructure for type analysis (`DependencyGraph`)
- Type usage analysis and tracking (`TypeUsage`, `type_usage.rs`)
- Transformation utilities for schema processing (`transforms.rs`)
- CLI subcommands architecture (`generate`, `list`)
- Response enum generation (`ResponseEnumDef`, `ResponseVariant`)
- Operation filtering support (`--only`, `--exclude` flags)
- Color and theme options for terminal output (`--color`, `--theme`)
- `--all-schemas` flag to generate all schemas regardless of operation references
- Generation statistics tracking for structs, enums, and type aliases
- Merged schema caching for performance optimization

### Changed

- Refactored error analyzer to use dependency graph for traversal
- Reorganized CLI into focused `ui` module with submodules
- Improved operation filtering and schema inclusion logic
- Enhanced discriminator field handling with `FieldProcessingContext`
- Extracted common attribute names into `constants.rs` module

### Fixed

- Improved error handling in code generation
- Enhanced null schema handling and optionality detection

## [0.12.0] - 2025-11-08

### Added

- Error schema analysis module (`ErrorAnalyzer`)
- Tracking of success and error response types in `OperationInfo`
- Deep and circular type reference analysis for error schemas
- Comprehensive error analyzer unit tests

### Changed

- Refactored type priority logic to prioritize Struct over DiscriminatedEnum and Enum
- Improved null schema handling with nullable object detection
- Enhanced field type optionality determination
- Major README rewrite with expanded documentation and examples
- Promoted `chrono` from dev-dependency to regular dependency

## [0.11.0] - 2025-11-06

### Added

- Support for inline enums in schemas
- Enhanced discriminator handling in `SchemaConverter`
- `mdformat` support for documentation comments
- Improved validation logic in code generation

### Changed

- Refactored schema management and dependency graph structures
- Enhanced type reference handling throughout converters
- Optimized sanitization functions for better performance
- Improved operation and schema converter clarity

## [0.8.0] - 2025-10-26

### Added

- Query parameter support in path rendering
- Percent encoding for query parameters
- Management of optional and array parameters
- Handling of multiple query parameter types

### Changed

- Simplified path rendering logic
- Enhanced parameter management in OpenAPI generator
- Refactored parameter handling for improved clarity
- Updated dependencies
- Removed outdated headers to streamline codebase

## [0.7.0] - 2025-10-26

### Added

- Header support in OpenAPI generation process

### Fixed

- Issues with `oneOf` discriminator handling
- Empty type generation problems
- Default discriminator values in generated code

### Changed

- Improved serialization with `serde_with`
- Applied clippy pedantic fixes throughout codebase

## [0.6.0] - 2025-10-24

### Changed

- Refactored codebase into multi-crate workspace structure
- Split into `oas3-gen` (CLI) and `oas3-gen-support` (runtime library)
- General code cleanup and organization improvements

### Fixed

- Issues with `allOf` functionality and schema merging

## [0.5.1] - 2025-10-21

### Changed

- Upgraded multiple dependencies
- Enhanced `allOf` schema conversion logic
- Improved recursive collection of properties and required fields
- More accurate schema representation for merged schemas

## [0.5.0] - 2025-10-21

### Added

- Visibility control for generated types (`--visibility` flag)
- Support for `public`, `crate`, and `file` visibility levels

### Changed

- Code cleanup to enhance readability and maintainability
- Improved code generation structure

## [0.4.0] - 2025-10-20

### Changed

- Changed `SchemaGraph` visibility to `pub(crate)` for better encapsulation
- Introduced `extract_ref_from_obj_ref` method for cleaner reference extraction
- Simplified dependency collection with direct recursion
- Updated `doc_comment_lines` utility to `pub(crate)`
- Replaced `CodeGenerator` and `SchemaConverter` with `Orchestrator` in main flow
- Enhanced cycle detection reporting

## [0.3.0] - 2025-10-20

### Changed

- Refactored schema dependency collection to use iterative approach with work queue
- Prevents stack overflow on deeply nested schemas
- Enhanced reserved identifier handling with sanitization function
- Improved logic for converting names to valid Rust identifiers

### Added

- Comprehensive tests for field and type name conversions
- Tests for reserved keyword handling and negative prefixes
- Integration tests for round-trip serialization/deserialization with Serde
- Validation of generated code against OpenAPI specifications

## [0.2.0] - 2025-10-19

### Added

- `uuid` dependency for UUID type support
- `indexmap` dependency for ordered map handling
- New fields for `FieldDef` and `TypeRef`: `read_only`, `write_only`, `deprecated`, `multiple_of`, `unique_items`
- Logic to handle `unique_items` in array types
- Schema name sanitization for valid Rust identifiers
- Type usage map for improved code generation
- Chrono support for `serde_json` date/time parsing

### Changed

- Refactored `generator.rs` with enhanced field and type definitions
- Adjusted type generation based on validation attributes
- Updated `reserved.rs` with better identifier sanitization
- Modified `main.rs` to build type usage map

### Removed

- `futures` and related unused packages

## [0.1.0] - 2025-10-18

### Added

- Initial release of OpenAPI 3.1 to Rust code generator
- Support for generating Rust types from OpenAPI specifications
- Struct, enum, and type alias generation
- Validation attributes using `validator` crate
- Serde serialization support
- Schema dependency tracking and cycle detection
- CLI interface with `clap`
- Support for `oneOf`, `anyOf`, and `allOf` schema combinations
- Discriminated enum generation with custom macro
- Request and response type generation from operations

[0.22.3]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.22.2...release/0.22.3
[0.22.2]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.22.0...release/0.22.2
[0.22.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.20.0...release/0.22.0
[0.20.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.17.0...release/0.20.0
[0.17.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.16.0...release/0.17.0
[0.16.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.14.0...release/0.16.0
[0.14.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.12.0...release/0.14.0
[0.12.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.11.0...release/0.12.0
[0.11.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.8.0...release/0.11.0
[0.8.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.5.1...v0.6.0
[0.5.1]: https://github.com/eklipse2k8/oas3-gen/compare/v0.5.0...release/0.5.1
[0.5.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/eklipse2k8/oas3-gen/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/eklipse2k8/oas3-gen/compare/release/0.1.0...v0.2.0
[0.1.0]: https://github.com/eklipse2k8/oas3-gen/releases/tag/release/0.1.0
