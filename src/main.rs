use crate::generator::{CodeGenerator, OperationConverter, SchemaConverter, SchemaGraph};
use clap::Parser;
use std::path::PathBuf;

mod generator;

/// OpenAPI to Rust code generator
///
/// Generates Rust type definitions from OpenAPI 3.x specifications with validation,
/// serde serialization, and comprehensive documentation.
#[derive(Parser, Debug)]
#[command(name = "openapi-gen")]
#[command(author, version, about, long_about = None)]
struct Cli {
  /// Path to the OpenAPI JSON specification file
  #[arg(short, long, value_name = "FILE")]
  input: PathBuf,

  /// Path where the generated Rust code will be written
  #[arg(short, long, value_name = "FILE")]
  output: PathBuf,

  /// Enable verbose output with detailed progress information
  #[arg(short, long, default_value_t = false)]
  verbose: bool,

  /// Suppress non-essential output (errors only)
  #[arg(short, long, default_value_t = false)]
  quiet: bool,
}

macro_rules! log_info {
  ($cli:expr, $($arg:tt)*) => {
    if !$cli.quiet {
      println!($($arg)*);
    }
  };
}

macro_rules! log_verbose {
  ($cli:expr, $($arg:tt)*) => {
    if $cli.verbose && !$cli.quiet {
      println!($($arg)*);
    }
  };
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();

  log_info!(cli, "Loading OpenAPI spec from: {}", cli.input.display());
  let file_content = tokio::fs::read_to_string(&cli.input).await?;
  let spec = oas3::from_json(file_content)?;

  log_info!(cli, "Building schema graph...");
  let mut graph = SchemaGraph::new(spec)?;
  log_info!(cli, "Found {} schemas", graph.schema_names().len());

  log_verbose!(cli, "\nBuilding dependency graph...");
  graph.build_dependencies();

  log_verbose!(cli, "Detecting cycles...");
  let cycles = graph.detect_cycles();
  if !cycles.is_empty() {
    log_info!(cli, "Found {} cycles:", cycles.len());
    for (i, cycle) in cycles.iter().enumerate() {
      log_verbose!(cli, "  Cycle {}: {}", i + 1, cycle.join(" -> "));
    }
  } else {
    log_verbose!(cli, "No cycles detected!");
  }

  log_info!(cli, "\nConverting schemas to Rust AST...");
  let schema_converter = SchemaConverter::new(&graph);
  let mut rust_types = Vec::new();

  for schema_name in graph.schema_names() {
    if let Some(schema) = graph.get_schema(schema_name) {
      match schema_converter.convert_schema(schema_name, schema) {
        Ok(types) => rust_types.extend(types),
        Err(e) => eprintln!("Warning: Failed to convert schema {}: {}", schema_name, e),
      }
    }
  }

  log_info!(cli, "Converted {} schema types", rust_types.len());

  // Convert operations to request/response types
  log_info!(cli, "\nConverting operations to Rust AST...");
  let operation_converter = OperationConverter::new(&schema_converter, graph.spec());
  let mut operations_info = Vec::new();

  if let Some(ref paths) = graph.spec().paths {
    let mut path_entries: Vec<_> = paths.iter().collect();
    path_entries.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (path, path_item) in path_entries {
      let mut methods: Vec<_> = path_item.methods().into_iter().collect();
      methods.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));

      for (method, operation) in methods {
        let method_str = method.as_str();
        let operation_id = operation.operation_id.as_deref().unwrap_or("unknown");

        match operation_converter.convert_operation(operation_id, method_str, path, operation) {
          Ok((types, op_info)) => {
            rust_types.extend(types);
            operations_info.push(op_info);
          }
          Err(e) => {
            eprintln!("Warning: Failed to convert operation {} {}: {}", method_str, path, e);
          }
        }
      }
    }
  }

  log_verbose!(cli, "Converted {} operations", operations_info.len());
  log_info!(cli, "Total types generated: {}", rust_types.len());

  log_info!(cli, "\nGenerating Rust code...");
  let code = CodeGenerator::generate(&rust_types);

  log_info!(cli, "Formatting code...");
  let syntax_tree = syn::parse2(code)?;
  let mut formatted = prettyplease::unparse(&syntax_tree);

  formatted = format!(
    "// AUTO-GENERATED CODE - DO NOT EDIT!\n// Generated from OpenAPI spec: {}\n\n{}\n\nfn main() {{}}\n",
    cli.input.display(),
    formatted
  );

  log_info!(cli, "Writing to output file: {}", cli.output.display());

  // Create parent directory if it doesn't exist
  if let Some(parent) = cli.output.parent() {
    tokio::fs::create_dir_all(parent).await?;
  }

  tokio::fs::write(&cli.output, formatted).await?;

  log_info!(cli, "\n✓ Successfully generated Rust types!");
  log_info!(cli, "  Input:  {}", cli.input.display());
  log_info!(cli, "  Output: {}", cli.output.display());
  log_info!(cli, "  Types:  {}", rust_types.len());
  if !cycles.is_empty() {
    log_info!(cli, "  Cycles: {}", cycles.len());
  }

  Ok(())
}
