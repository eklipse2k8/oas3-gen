# Codegen Fragments

`codegen/` turns AST nodes into Rust source. A fragment is a small struct implementing `ToTokens`
that owns one emission concern and composes with others.

Fragment names and their nesting are in the source and drift constantly — don't work from a list:

```bash
rg 'struct \w+Fragment' crates/oas3-gen/src/generator/codegen/
rg 'impl ToTokens for' crates/oas3-gen/src/generator/codegen/<file>.rs
```

Because fragments compose, one `quote!` block rarely shows the final output. Confirm emitted
shape against `crates/oas3-gen/fixtures/`, not by reading a single fragment.

## Dispatch map

The one thing not obvious from the tree — which AST node reaches which top-level fragment:

| AST input | Fragment | File | Output |
|---|---|---|---|
| `Vec<RustType>` | `TypesFragment` | `types.rs` | whole `types.rs` (imports, constants, types) |
| `RustType` | `TypeFragment` | `types.rs` | dispatches on variant to the rows below |
| `StructDef` | `StructFragment` | `structs.rs` | struct + impls + `TryFrom` for `HeaderMap` |
| `EnumDef` | `EnumFragment` | `enums.rs` | value enum, `Display`, optional case-insensitive deser |
| `DiscriminatedEnumDef` | `DiscriminatedEnumFragment` | `enums.rs` | tagged union + hand-rolled serde |
| `ResponseEnumDef` | `ResponseEnumFragment` | `enums.rs` | the one generic `ApiResponse<Value, Failure>` response enum, both targets |
| `PayloadUnionDef` | `PayloadUnionFragment` | `enums.rs` | untagged union of body types sharing a status class |
| `TypeAliasDef` | `TypeAliasFragment` | `type_aliases.rs` | `type Alias = Target;` |
| `Vec<OperationInfo>` | `ClientFragment` | `client.rs` | `impl Client` with async methods |
| `ServerRequestTraitDef` | `ServerGenerator` | `server.rs` | trait + axum handlers (response enum match arms) + router |

Shared helpers: `attributes.rs` (derives, serde, validation, docs), `methods.rs` (helper methods,
parameters), `constants.rs` (regex statics, header names), `coercion.rs` (JSON literal -> Rust
literal), `headers.rs`, `http.rs`, `mod_file.rs`, `cargo_manifest.rs` (`--workspace` output).

## Conventions

- One concern per fragment; compose by holding references, never by inheritance-style layering.
- Thread `Visibility` through rather than hard-coding `pub`.
- Return empty `quote! {}` for absent/empty cases instead of branching at the call site.
- Spec order for schema/field/variant/header output; sorted only for canonical output like
  grouped imports and derives. See [coding-standards.md](coding-standards.md#collections).
- `SchemaCodeGenerator` (`mod.rs`) owns `CodegenConfig` by value and reads policy off it
  (`config.collection_types`, `config.target`). The `Rc<ConverterContext>` sharing pattern belongs
  to `converter/`, not here.
- Method generation goes through `HelperMethodFragment<Parts>` and the `HelperMethodParts` trait
  (`methods.rs`) so struct and enum methods share one implementation.
