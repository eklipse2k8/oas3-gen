#![allow(dead_code)]

use std::{
  cmp::Ordering,
  collections::{BTreeMap, BTreeSet},
};

use any_ascii::any_ascii;
use inflections::{Inflect, case::to_pascal_case};
use oas3::{
  Spec,
  spec::{ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, Schema, SchemaType, SchemaTypeSet},
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use regex::Regex;
use serde_json::Number;

pub fn doc_comment_lines(input: &str) -> Vec<String> {
  let normalized = input.replace("\\n", "\n");
  normalized
    .lines()
    .map(|line| {
      if line.is_empty() {
        "/// ".to_string()
      } else {
        format!("/// {}", line)
      }
    })
    .collect()
}

pub fn doc_comment_block(input: &str) -> String {
  doc_comment_lines(input).join("\n")
}

/// Convert a schema name to a valid Rust identifier (for field names)
pub fn to_rust_ident(name: &str) -> String {
  let cleaned = name.replace(['-', '.', ' '], "_");

  // Check if it's a Rust keyword and prefix with r# if needed
  match cleaned.as_str() {
    "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if"
    | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self"
    | "Self" | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while"
    | "async" | "await" | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override" | "priv"
    | "typeof" | "unsized" | "virtual" | "yield" | "try" => format!("r#{}", cleaned),
    _ => cleaned,
  }
}

/// Convert a schema name to a valid Rust type name (PascalCase)
pub fn to_rust_type_name(name: &str) -> String {
  let pascal = any_ascii(name).to_pascal_case();

  // Check if it's a Rust keyword and prefix with r# if needed
  match pascal.as_str() {
    "Self" | "Type" => format!("r#{}", pascal),
    _ => pascal,
  }
}

/// Detect if all field names follow a consistent naming pattern
/// Returns the serde rename_all value if consistent, None otherwise
pub fn detect_naming_pattern(fields: &[(String, String)]) -> Option<&'static str> {
  if fields.is_empty() {
    return None;
  }

  // Check if all fields are snake_case
  let all_snake_case = fields.iter().all(|(original, rust_name)| {
    original.contains('_') || original.contains('-') && to_rust_ident(original) == *rust_name
  });

  if all_snake_case {
    return Some("snake_case");
  }

  // Check if all fields are camelCase
  let all_camel_case = fields.iter().all(|(original, _)| {
    original.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) && original.chars().any(|c| c.is_uppercase())
  });

  if all_camel_case {
    return Some("camelCase");
  }

  None
}

#[derive(Debug)]
pub struct SchemaGraph {
  /// All schemas from the OpenAPI spec
  schemas: BTreeMap<String, ObjectSchema>,
  /// Dependency graph: schema_name -> [schemas it references]
  dependencies: BTreeMap<String, BTreeSet<String>>,
  /// Schemas that are part of cycles
  cyclic_schemas: BTreeSet<String>,
  /// Reference to the original spec for resolution
  spec: Spec,
}

impl SchemaGraph {
  pub fn new(spec: Spec) -> anyhow::Result<Self> {
    let mut graph = Self {
      schemas: BTreeMap::new(),
      dependencies: BTreeMap::new(),
      cyclic_schemas: BTreeSet::new(),
      spec,
    };

    // Extract all schemas from components/schemas
    if let Some(components) = &graph.spec.components {
      for (name, schema_ref) in &components.schemas {
        if let Ok(schema) = schema_ref.resolve(&graph.spec) {
          graph.schemas.insert(name.clone(), schema);
        }
      }
    }

    Ok(graph)
  }

  /// Get a schema by name
  pub fn get_schema(&self, name: &str) -> Option<&ObjectSchema> {
    self.schemas.get(name)
  }

  /// Get all schema names
  pub fn schema_names(&self) -> Vec<&String> {
    self.schemas.keys().collect()
  }

  /// Get the spec reference
  pub fn spec(&self) -> &Spec {
    &self.spec
  }

  /// Extract schema name from a $ref string
  pub fn extract_ref_name(ref_string: &str) -> Option<String> {
    // Format: "#/components/schemas/SchemaName"
    ref_string.strip_prefix("#/components/schemas/").map(|s| s.to_string())
  }

  /// Build the dependency graph by analyzing all schema references
  pub fn build_dependencies(&mut self) {
    let schema_names: Vec<String> = self.schemas.keys().cloned().collect();

    for schema_name in schema_names {
      let mut deps = BTreeSet::new();
      if let Some(schema) = self.schemas.get(&schema_name) {
        self.collect_dependencies(schema, &mut deps);
      }
      self.dependencies.insert(schema_name, deps);
    }
  }

  /// Recursively collect all schema dependencies from a schema
  fn collect_dependencies(&self, schema: &ObjectSchema, deps: &mut BTreeSet<String>) {
    // Check properties
    for prop_schema in schema.properties.values() {
      // Try to resolve the property schema and extract dependencies
      if let Ok(resolved) = prop_schema.resolve(&self.spec) {
        // Check if this is a reference to another schema by looking at the title
        if let Some(ref title) = resolved.title {
          deps.insert(title.clone());
        }
        // Recursively collect from inline schemas
        self.collect_dependencies(&resolved, deps);
      }
    }

    // Check oneOf
    for one_of_schema in &schema.one_of {
      if let Ok(resolved) = one_of_schema.resolve(&self.spec) {
        if let Some(ref title) = resolved.title {
          deps.insert(title.clone());
        }
        self.collect_dependencies(&resolved, deps);
      }
    }

    // Check anyOf
    for any_of_schema in &schema.any_of {
      if let Ok(resolved) = any_of_schema.resolve(&self.spec) {
        if let Some(ref title) = resolved.title {
          deps.insert(title.clone());
        }
        self.collect_dependencies(&resolved, deps);
      }
    }

    // Check allOf
    for all_of_schema in &schema.all_of {
      if let Ok(resolved) = all_of_schema.resolve(&self.spec) {
        if let Some(ref title) = resolved.title {
          deps.insert(title.clone());
        }
        self.collect_dependencies(&resolved, deps);
      }
    }
  }

  /// Detect cycles in the schema dependency graph using DFS
  pub fn detect_cycles(&mut self) -> Vec<Vec<String>> {
    let mut visited = BTreeSet::new();
    let mut rec_stack = BTreeSet::new();
    let mut cycles = Vec::new();
    let mut path = Vec::new();

    let schema_names: Vec<String> = self.schemas.keys().cloned().collect();

    for schema_name in schema_names {
      if !visited.contains(&schema_name) {
        self.dfs_detect_cycle(&schema_name, &mut visited, &mut rec_stack, &mut path, &mut cycles);
      }
    }

    // Mark all schemas involved in cycles
    for cycle in &cycles {
      for schema_name in cycle {
        self.cyclic_schemas.insert(schema_name.clone());
      }
    }

    cycles
  }

  /// DFS helper for cycle detection
  fn dfs_detect_cycle(
    &self,
    node: &str,
    visited: &mut BTreeSet<String>,
    rec_stack: &mut BTreeSet<String>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
  ) {
    visited.insert(node.to_string());
    rec_stack.insert(node.to_string());
    path.push(node.to_string());

    if let Some(deps) = self.dependencies.get(node) {
      for dep in deps {
        if !visited.contains(dep) {
          self.dfs_detect_cycle(dep, visited, rec_stack, path, cycles);
        } else if rec_stack.contains(dep) {
          // Found a cycle! Extract the cycle from the path
          if let Some(cycle_start) = path.iter().position(|n| n == dep) {
            let cycle: Vec<String> = path[cycle_start..].to_vec();
            cycles.push(cycle);
          }
        }
      }
    }

    path.pop();
    rec_stack.remove(node);
  }

  /// Check if a schema is part of a cycle
  pub fn is_cyclic(&self, schema_name: &str) -> bool {
    self.cyclic_schemas.contains(schema_name)
  }

  /// Get dependencies of a schema
  pub fn get_dependencies(&self, schema_name: &str) -> Option<&BTreeSet<String>> {
    self.dependencies.get(schema_name)
  }
}

#[derive(Debug, Clone)]
pub enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
}

impl RustType {
  pub fn type_name(&self) -> &str {
    match self {
      RustType::Struct(def) => &def.name,
      RustType::Enum(def) => &def.name,
      RustType::TypeAlias(def) => &def.name,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[derive(Debug, Clone)]
pub struct OperationInfo {
  pub operation_id: String,
  pub method: String,
  pub path: String,
  pub summary: Option<String>,
  pub description: Option<String>,
  pub request_type: Option<String>,
  pub response_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StructDef {
  pub name: String,
  pub docs: Vec<String>,
  pub fields: Vec<FieldDef>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
  pub name: String,
  pub docs: Vec<String>,
  pub rust_type: TypeRef,
  pub optional: bool,
  pub serde_attrs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
  pub base_type: String,
  pub boxed: bool,
  pub nullable: bool,
  pub is_array: bool,
}

impl TypeRef {
  pub fn new(base_type: impl Into<String>) -> Self {
    Self {
      base_type: base_type.into(),
      boxed: false,
      nullable: false,
      is_array: false,
    }
  }

  pub fn with_option(mut self) -> Self {
    self.nullable = true;
    self
  }

  pub fn with_vec(mut self) -> Self {
    self.is_array = true;
    self
  }

  pub fn with_boxed(mut self) -> Self {
    self.boxed = true;
    self
  }

  /// Get the full Rust type string
  pub fn to_rust_type(&self) -> String {
    let mut result = self.base_type.clone();

    if self.boxed {
      result = format!("Box<{}>", result);
    }

    if self.is_array {
      result = format!("Vec<{}>", result);
    }

    if self.nullable {
      result = format!("Option<{}>", result);
    }

    result
  }
}

#[derive(Debug, Clone)]
pub struct EnumDef {
  pub name: String,
  pub docs: Vec<String>,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VariantDef {
  pub name: String,
  pub docs: Vec<String>,
  pub content: VariantContent,
  pub serde_attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum VariantContent {
  Unit,
  Tuple(Vec<TypeRef>),
  Struct(Vec<FieldDef>),
}

#[derive(Debug, Clone)]
pub struct TypeAliasDef {
  pub name: String,
  pub docs: Vec<String>,
  pub target: TypeRef,
}

// ============================================================================
// Schema to AST Converter
// ============================================================================

pub struct SchemaConverter<'a> {
  graph: &'a SchemaGraph,
}

impl<'a> SchemaConverter<'a> {
  pub fn new(graph: &'a SchemaGraph) -> Self {
    Self { graph }
  }

  /// Convert a schema to Rust type definitions
  /// Returns the main type and any inline types that were generated
  pub fn convert_schema(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<Vec<RustType>> {
    // Determine the type of Rust definition we need to create

    // Check if this is an enum (oneOf/anyOf)
    if !schema.one_of.is_empty() {
      return Ok(vec![self.convert_one_of_enum(name, schema)?]);
    }

    if !schema.any_of.is_empty() {
      return Ok(vec![self.convert_any_of_enum(name, schema)?]);
    }

    // Check if this is a simple enum (string with enum values)
    if !schema.enum_values.is_empty() {
      return Ok(vec![self.convert_simple_enum(name, schema, &schema.enum_values)?]);
    }

    // Check if this is a struct (object with properties)
    if !schema.properties.is_empty() {
      let (main_type, inline_types) = self.convert_struct(name, schema)?;
      let mut all_types = vec![main_type];
      all_types.extend(inline_types);
      return Ok(all_types);
    }

    // Otherwise, might be a type alias or something we can skip
    Ok(vec![])
  }

  /// Convert a schema with oneOf into a Rust enum
  fn convert_one_of_enum(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<RustType> {
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    // Get discriminator property name if present
    let discriminator_property = schema.discriminator.as_ref().map(|d| d.property_name.as_str());

    for (i, variant_schema_ref) in schema.one_of.iter().enumerate() {
      if let Ok(variant_schema) = variant_schema_ref.resolve(self.graph.spec()) {
        // Skip null variants - they're handled by making the field Option<T>
        if variant_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          continue;
        }

        // Generate a good variant name
        let mut variant_name = if let Some(ref title) = variant_schema.title {
          to_pascal_case(title)
        } else {
          // Infer name from type
          self.infer_variant_name(&variant_schema, i)
        };

        // Ensure uniqueness
        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        let docs = variant_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();

        // Determine the variant content based on the schema type
        // For discriminated unions (with tag), we MUST use struct variants (serde requirement)
        // For non-discriminated (untagged), we can use tuple variants to avoid duplication
        let content = if discriminator_property.is_some() {
          // Has discriminator - must use struct variant for serde(tag) to work
          if !variant_schema.properties.is_empty() {
            let fields = self.convert_fields_with_exclusions(&variant_schema, discriminator_property)?;
            VariantContent::Struct(fields)
          } else {
            // Primitive or non-object - tuple variant
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            VariantContent::Tuple(vec![type_ref])
          }
        } else {
          // No discriminator - can use tuple variants to avoid duplication
          if let Some(ref title) = variant_schema.title {
            if self.graph.get_schema(title).is_some() {
              // Reference to existing schema - use tuple variant
              let type_ref = TypeRef::new(to_rust_type_name(title));
              VariantContent::Tuple(vec![type_ref])
            } else if !variant_schema.properties.is_empty() {
              // Inline object - struct variant
              let fields = self.convert_fields(&variant_schema)?;
              VariantContent::Struct(fields)
            } else {
              // Other types - tuple variant
              let type_ref = self.schema_to_type_ref(&variant_schema)?;
              VariantContent::Tuple(vec![type_ref])
            }
          } else if !variant_schema.properties.is_empty() {
            // Anonymous object - inline struct variant
            let fields = self.convert_fields(&variant_schema)?;
            VariantContent::Struct(fields)
          } else {
            // Primitive - tuple variant
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            VariantContent::Tuple(vec![type_ref])
          }
        };

        variants.push(VariantDef {
          name: to_rust_ident(&variant_name),
          docs,
          content,
          serde_attrs: vec![],
        });
      }
    }

    // Check if there's a discriminator
    let discriminator = schema.discriminator.as_ref().map(|d| d.property_name.clone());

    Ok(RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec![],
    }))
  }

  /// Convert a schema with anyOf into an untagged Rust enum
  fn convert_any_of_enum(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<RustType> {
    // Check if this is a string enum with const values pattern (common for forward-compatible enums)
    let has_freeform_string = schema.any_of.iter().any(|s| {
      if let Ok(resolved) = s.resolve(self.graph.spec()) {
        resolved.const_value.is_none() && resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String))
      } else {
        false
      }
    });

    let const_values: Vec<_> = schema
      .any_of
      .iter()
      .filter_map(|s| {
        if let Ok(resolved) = s.resolve(self.graph.spec()) {
          resolved
            .const_value
            .as_ref()
            .map(|v| (v.clone(), resolved.description.clone()))
        } else {
          None
        }
      })
      .collect();

    // Special case: freeform string + const values = forward-compatible enum
    if has_freeform_string && !const_values.is_empty() {
      return self.convert_string_enum_with_catchall(name, schema, &const_values);
    }

    // Otherwise, treat as a regular untagged enum
    let mut variants = Vec::new();
    let mut seen_names = BTreeSet::new();

    for (i, variant_schema_ref) in schema.any_of.iter().enumerate() {
      if let Ok(variant_schema) = variant_schema_ref.resolve(self.graph.spec()) {
        // Skip null variants - they're handled by making the field Option<T>
        if variant_schema.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
          continue;
        }

        // Generate a good variant name
        let mut variant_name = if let Some(ref title) = variant_schema.title {
          to_pascal_case(title)
        } else {
          // Infer name from type
          self.infer_variant_name(&variant_schema, i)
        };

        // Ensure uniqueness
        if seen_names.contains(&variant_name) {
          variant_name = format!("{}{}", variant_name, i);
        }
        seen_names.insert(variant_name.clone());

        let docs = variant_schema
          .description
          .as_ref()
          .map(|d| doc_comment_lines(d))
          .unwrap_or_default();

        // Determine variant content - prefer tuple variants for existing schemas
        let content = if let Some(ref title) = variant_schema.title {
          // If this variant has a title and matches an existing schema, use tuple variant
          if self.graph.get_schema(title).is_some() {
            let type_ref = TypeRef::new(to_rust_type_name(title));
            VariantContent::Tuple(vec![type_ref])
          } else if !variant_schema.properties.is_empty() {
            // Inline object without matching schema - create struct variant
            let fields = self.convert_fields(&variant_schema)?;
            VariantContent::Struct(fields)
          } else {
            // Other types - tuple variant
            let type_ref = self.schema_to_type_ref(&variant_schema)?;
            VariantContent::Tuple(vec![type_ref])
          }
        } else if !variant_schema.properties.is_empty() {
          // Anonymous object - create inline struct variant
          let fields = self.convert_fields(&variant_schema)?;
          VariantContent::Struct(fields)
        } else {
          // Not an object - create tuple variant wrapping the type
          let type_ref = self.schema_to_type_ref(&variant_schema)?;
          VariantContent::Tuple(vec![type_ref])
        };

        variants.push(VariantDef {
          name: to_rust_ident(&variant_name),
          docs,
          content,
          serde_attrs: vec![],
        });
      }
    }

    Ok(RustType::Enum(EnumDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator: None,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec!["untagged".into()],
    }))
  }

  /// Convert a string enum with const values + a catch-all for unknown strings
  fn convert_string_enum_with_catchall(
    &self,
    name: &str,
    schema: &ObjectSchema,
    const_values: &[(serde_json::Value, Option<String>)],
  ) -> anyhow::Result<RustType> {
    let mut variants = Vec::new();

    // Add a variant for each const value
    for (value, description) in const_values {
      if let Some(str_val) = value.as_str() {
        // Convert the const value to a variant name
        // e.g., "claude-3-7-sonnet-latest" -> "Claude37SonnetLatest"
        let variant_name = to_pascal_case(&str_val.replace(['-', '.'], "_"));

        let docs = description.as_ref().map(|d| doc_comment_lines(d)).unwrap_or_default();

        variants.push(VariantDef {
          name: variant_name,
          docs,
          content: VariantContent::Unit,
          serde_attrs: vec![format!("rename = \"{}\"", str_val)],
        });
      }
    }

    // Add the catch-all variant for unknown strings
    variants.push(VariantDef {
      name: "Other".to_string(),
      docs: vec!["/// Any other string value".to_string()],
      content: VariantContent::Tuple(vec![TypeRef::new("String")]),
      serde_attrs: vec!["untagged".to_string()],
    });

    Ok(RustType::Enum(EnumDef {
      name: to_rust_ident(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator: None,
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "PartialEq".into(),
        "Eq".into(),
        "Serialize".into(),
        "Deserialize".into(),
      ],
      serde_attrs: vec![],
    }))
  }

  /// Infer a variant name from the schema type
  fn infer_variant_name(&self, schema: &ObjectSchema, index: usize) -> String {
    // Check if it's an enum
    if !schema.enum_values.is_empty() {
      return "Enum".to_string();
    }

    // Check the schema type
    if let Some(ref schema_type) = schema.schema_type {
      match schema_type {
        SchemaTypeSet::Single(typ) => match typ {
          SchemaType::String => "String".to_string(),
          SchemaType::Number => "Number".to_string(),
          SchemaType::Integer => "Integer".to_string(),
          SchemaType::Boolean => "Boolean".to_string(),
          SchemaType::Array => "Array".to_string(),
          SchemaType::Object => "Object".to_string(),
          SchemaType::Null => "Null".to_string(),
        },
        SchemaTypeSet::Multiple(_) => "Mixed".to_string(),
      }
    } else {
      // Fallback
      format!("Variant{}", index)
    }
  }

  /// Convert a simple string enum
  fn convert_simple_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    enum_values: &[serde_json::Value],
  ) -> anyhow::Result<RustType> {
    let mut variants = Vec::new();

    for value in enum_values {
      if let Some(str_val) = value.as_str() {
        let variant_name = to_pascal_case(str_val);
        variants.push(VariantDef {
          name: variant_name,
          docs: vec![],
          content: VariantContent::Unit,
          serde_attrs: vec![format!("rename = \"{}\"", str_val)],
        });
      }
    }

    Ok(RustType::Enum(EnumDef {
      name: to_rust_ident(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      variants,
      discriminator: None,
      derives: vec!["Debug".into(), "Clone".into(), "Serialize".into(), "Deserialize".into()],
      serde_attrs: vec![],
    }))
  }

  /// Convert an object schema to a Rust struct
  /// Returns the struct and any inline types that were generated
  fn convert_struct(&self, name: &str, schema: &ObjectSchema) -> anyhow::Result<(RustType, Vec<RustType>)> {
    let (mut fields, inline_types) = self.convert_fields_with_inline_types(name, schema)?;

    // Detect if all fields follow a consistent naming pattern for serde rename_all
    let mut serde_attrs = vec![];

    // Collect fields that have rename attributes
    let renamed_fields: Vec<_> = fields
      .iter()
      .filter_map(|f| {
        f.serde_attrs
          .iter()
          .find(|attr| attr.starts_with("rename = "))
          .map(|attr| {
            // Extract the original name from rename = "original-name"
            let start = attr.find('"').unwrap() + 1;
            let end = attr.rfind('"').unwrap();
            attr[start..end].to_string()
          })
      })
      .collect();

    // If we have renamed fields, check if they all follow a consistent pattern
    if !renamed_fields.is_empty() {
      let all_kebab = renamed_fields.iter().all(|name| name.contains('-'));

      if all_kebab {
        // Use rename_all = "kebab-case" for all fields
        // serde will automatically convert snake_case Rust names to kebab-case
        serde_attrs.push("rename_all = \"kebab-case\"".to_string());

        // Remove individual rename attributes since rename_all handles it
        for field in &mut fields {
          field.serde_attrs.retain(|attr| !attr.starts_with("rename = "));
        }
      }
    }

    // Only add serde(default) at struct level if ALL fields have defaults or are Option/Vec
    // Otherwise we get compilation errors when trying to Default::default() complex types
    let all_fields_defaultable = fields.iter().all(|f| {
      f.default_value.is_some()
        || f.rust_type.nullable
        || f.rust_type.is_array
        || matches!(
          f.rust_type.base_type.as_str(),
          "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "serde_json::Value"
        )
    });

    if all_fields_defaultable && fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push("default".to_string());
    }

    let struct_type = RustType::Struct(StructDef {
      name: to_rust_type_name(name),
      docs: schema
        .description
        .as_ref()
        .map(|d| doc_comment_lines(d))
        .unwrap_or_default(),
      fields,
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "Serialize".into(),
        "Deserialize".into(),
        "Validate".into(),
      ],
      serde_attrs,
    });

    Ok((struct_type, inline_types))
  }

  fn extract_validation_pattern<'s>(&self, prop_name: &str, schema: &'s ObjectSchema) -> Option<&'s String> {
    match (schema.schema_type.as_ref(), schema.pattern.as_ref()) {
      (Some(SchemaTypeSet::Single(SchemaType::String)), Some(pattern)) => {
        if Regex::new(pattern).is_ok() {
          Some(pattern)
        } else {
          eprintln!(
            "Warning: Invalid regex pattern '{}' for property '{}'",
            pattern, prop_name
          );
          None
        }
      }
      _ => None,
    }
  }

  fn render_number(is_float: bool, num: &Number) -> String {
    if is_float {
      if num.to_string().contains(".") {
        num.to_string()
      } else {
        format!("{}.0", num)
      }
    } else {
      format!("{}i64", num.as_i64().unwrap_or_default())
    }
  }

  /// Extract validation attributes from an OpenAPI schema
  fn extract_validation_attrs(&self, _prop_name: &str, is_required: bool, schema: &ObjectSchema) -> Vec<String> {
    let mut attrs = Vec::new();

    // Handle format-based validation
    if let Some(ref format) = schema.format {
      match format.as_str() {
        "email" => attrs.push("email".to_string()),
        "uri" | "url" => attrs.push("url".to_string()),
        _ => {}
      }
    }

    if let Some(ref schema_type) = schema.schema_type {
      if matches!(
        schema_type,
        SchemaTypeSet::Single(SchemaType::Number) | SchemaTypeSet::Single(SchemaType::Integer)
      ) {
        let mut parts = Vec::<String>::new();
        let is_float = matches!(schema_type, SchemaTypeSet::Single(SchemaType::Number));

        // exclusive_minimum
        if let Some(exclusive_min) = schema
          .exclusive_minimum
          .as_ref()
          .map(|v| format!("exclusive_min = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_min);
        }

        // exclusive_maximum
        if let Some(exclusive_max) = schema
          .exclusive_maximum
          .as_ref()
          .map(|v| format!("exclusive_max = {}", Self::render_number(is_float, v)))
        {
          parts.push(exclusive_max);
        }

        // minimum
        if let Some(min) = schema
          .minimum
          .as_ref()
          .map(|v| format!("min = {}", Self::render_number(is_float, v)))
        {
          parts.push(min);
        }

        // maximum
        if let Some(max) = schema
          .maximum
          .as_ref()
          .map(|v| format!("max = {}", Self::render_number(is_float, v)))
        {
          parts.push(max);
        }

        if !parts.is_empty() {
          attrs.push(format!("range({})", parts.join(", ")));
        }
      }

      // string length validation
      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::String)) && schema.enum_values.is_empty() {
        if let (Some(min), Some(max)) = (schema.min_length, schema.max_length) {
          attrs.push(format!("length(min = {min}, max = {max})"));
        } else if let Some(min) = schema.min_length {
          attrs.push(format!("length(min = {min})"));
        } else if let Some(max) = schema.max_length {
          attrs.push(format!("length(max = {max})"));
        } else if is_required {
          // Require non-empty string for required fields
          attrs.push("length(min = 1)".to_string());
        }
      }

      // array length validation
      if matches!(schema_type, SchemaTypeSet::Single(SchemaType::Array)) {
        if let (Some(min), Some(max)) = (schema.min_items, schema.max_items) {
          attrs.push(format!("length(min = {min}, max = {max})"));
        } else if let Some(min) = schema.min_items {
          attrs.push(format!("length(min = {min})"));
        } else if let Some(max) = schema.max_items {
          attrs.push(format!("length(max = {max})"));
        }
      }
    }

    attrs
  }

  /// Extract default value from an OpenAPI schema
  fn extract_default_value(&self, schema: &ObjectSchema) -> Option<serde_json::Value> {
    schema.default.clone()
  }

  /// Convert schema properties to struct fields, excluding specified field names
  fn convert_fields_with_exclusions(
    &self,
    schema: &ObjectSchema,
    exclude_field: Option<&str>,
  ) -> anyhow::Result<Vec<FieldDef>> {
    let mut fields = Vec::new();

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      // Skip excluded fields (e.g., discriminator fields)
      if let Some(exclude) = exclude_field
        && prop_name == exclude
      {
        continue;
      }

      // Check if this is a direct $ref first
      let rust_type = if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
        // Extract type name directly from the reference
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          TypeRef::new(to_rust_type_name(&ref_name))
        } else {
          // Fallback to resolution
          if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
            self.schema_to_type_ref(&prop_schema)?
          } else {
            TypeRef::new("serde_json::Value")
          }
        }
      } else {
        // Inline schema - resolve and convert
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          self.schema_to_type_ref(&prop_schema)?
        } else {
          TypeRef::new("serde_json::Value")
        }
      };

      let is_required = schema.required.contains(prop_name);
      let optional = !is_required;

      let mut serde_attrs = vec![];
      // Add rename if the property name is not a valid Rust identifier or uses snake_case
      if prop_name.contains('-') || prop_name.contains('.') {
        serde_attrs.push(format!("rename = \"{}\"", prop_name));
      }

      // Add skip_serializing_if for optional fields
      if optional {
        serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
      }

      // Extract validation attributes and default value from resolved schema
      let (docs, validation_attrs, regex_validation, default_value) =
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          let docs = prop_schema
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default();
          let validation = self.extract_validation_attrs(prop_name, is_required, &prop_schema);
          let regex_validation = self.extract_validation_pattern(prop_name, &prop_schema);
          let default = self.extract_default_value(&prop_schema);
          (docs, validation, regex_validation.cloned(), default)
        } else {
          (vec![], vec![], None, None)
        };

      // Don't double-wrap: if the type is already nullable, don't wrap again
      let final_type = if optional && !rust_type.nullable {
        rust_type.with_option()
      } else {
        rust_type
      };

      fields.push(FieldDef {
        name: to_rust_ident(prop_name),
        docs,
        rust_type: final_type,
        optional,
        serde_attrs,
        validation_attrs,
        regex_validation,
        default_value,
      });
    }

    Ok(fields)
  }

  /// Convert schema properties to struct fields (convenience wrapper)
  fn convert_fields(&self, schema: &ObjectSchema) -> anyhow::Result<Vec<FieldDef>> {
    self.convert_fields_with_exclusions(schema, None)
  }

  /// Convert schema properties to struct fields, generating inline enum types for anyOf unions
  fn convert_fields_with_inline_types(
    &self,
    parent_name: &str,
    schema: &ObjectSchema,
  ) -> anyhow::Result<(Vec<FieldDef>, Vec<RustType>)> {
    let mut fields = Vec::new();
    let mut inline_types = Vec::new();

    let mut properties: Vec<_> = schema.properties.iter().collect();
    properties.sort_by(|(a, _), (b, _)| a.cmp(b));

    for (prop_name, prop_schema_ref) in properties {
      // Check if this is a direct $ref first
      let rust_type = if let ObjectOrReference::Ref { ref_path, .. } = prop_schema_ref {
        // Extract type name directly from the reference
        if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
          TypeRef::new(to_rust_type_name(&ref_name))
        } else {
          // Fallback to resolution
          if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
            self.schema_to_type_ref(&prop_schema)?
          } else {
            TypeRef::new("serde_json::Value")
          }
        }
      } else {
        // Inline schema - resolve and convert
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          // Special handling for inline anyOf unions
          // Check if this is just a nullable pattern (anyOf with null)
          let has_null = prop_schema.any_of.iter().any(|v| {
            if let Ok(resolved) = v.resolve(self.graph.spec()) {
              resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
            } else {
              false
            }
          });

          // If anyOf has null and exactly 2 variants, it's just an optional type

          if !prop_schema.any_of.is_empty() && has_null && prop_schema.any_of.len() == 2 {
            // Extract the non-null type
            let mut found_type = None;
            for variant_ref in &prop_schema.any_of {
              // Check if it's a $ref first (before resolving)
              if let ObjectOrReference::Ref { ref_path, .. } = variant_ref {
                if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
                  found_type = Some(TypeRef::new(to_rust_type_name(&ref_name)));
                  break;
                }
              } else if let Ok(resolved) = variant_ref.resolve(self.graph.spec())
                && resolved.schema_type != Some(SchemaTypeSet::Single(SchemaType::Null))
              {
                // Found the actual type - return it (it will be wrapped in Option later)
                found_type = Some(self.schema_to_type_ref(&resolved)?);
                break;
              }
            }
            // Use found type or fallback
            found_type.unwrap_or_else(|| self.schema_to_type_ref(&prop_schema).unwrap())
          } else if !prop_schema.any_of.is_empty()
            && (prop_schema.title.is_none()
              || prop_schema
                .title
                .as_ref()
                .map(|t| self.graph.get_schema(t).is_none())
                .unwrap_or(true))
          {
            // Generate inline enum for non-nullable anyOf unions
            let enum_name = format!("{}{}", parent_name, to_pascal_case(prop_name));
            let enum_type = self.convert_any_of_enum(&enum_name, &prop_schema)?;
            inline_types.push(enum_type);
            TypeRef::new(to_rust_type_name(&enum_name))
          } else {
            self.schema_to_type_ref(&prop_schema)?
          }
        } else {
          TypeRef::new("serde_json::Value")
        }
      };

      let is_required = schema.required.contains(prop_name);
      let optional = !is_required;

      let mut serde_attrs = vec![];
      // Add rename if the property name is not a valid Rust identifier or uses snake_case
      if prop_name.contains('-') || prop_name.contains('.') {
        serde_attrs.push(format!("rename = \"{}\"", prop_name));
      }

      // Add skip_serializing_if for optional fields
      if optional {
        serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
      }

      // Extract validation attributes and default value from resolved schema
      let (docs, validation_attrs, regex_validation, default_value) =
        if let Ok(prop_schema) = prop_schema_ref.resolve(self.graph.spec()) {
          let docs = prop_schema
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default();
          let required = schema.required.contains(prop_name);
          let validation = self.extract_validation_attrs(prop_name, required, &prop_schema);
          let regex_validation = self.extract_validation_pattern(prop_name, &prop_schema);
          let default = self.extract_default_value(&prop_schema);
          (docs, validation, regex_validation.cloned(), default)
        } else {
          (vec![], vec![], None, None)
        };

      // Don't double-wrap: if the type is already nullable, don't wrap again
      let final_type = if optional && !rust_type.nullable {
        rust_type.with_option()
      } else {
        rust_type
      };

      fields.push(FieldDef {
        name: to_rust_ident(prop_name),
        docs,
        rust_type: final_type,
        optional,
        serde_attrs,
        validation_attrs,
        regex_validation,
        default_value,
      });
    }

    Ok((fields, inline_types))
  }

  /// Convert an OpenAPI schema to a TypeRef (exposed for OperationConverter)
  pub fn schema_to_type_ref(&self, schema: &ObjectSchema) -> anyhow::Result<TypeRef> {
    // First priority: Check the schema type - if it has a concrete type, use that
    // This prevents title conflicts (e.g., a string field titled "Message" being confused with Message struct)
    if let Some(ref schema_type) = schema.schema_type {
      // If it has a concrete type AND properties/oneOf/anyOf, it might be a complex type
      // Only use title-based lookup for objects without explicit primitive types
      if !matches!(schema_type, SchemaTypeSet::Single(SchemaType::Object)) {
        // It's a primitive type - continue to primitive type handling below
      } else if let Some(ref title) = schema.title
        && self.graph.get_schema(title).is_some()
        && !schema.properties.is_empty()
      {
        // It's an object with a title that matches a schema and has properties
        let is_cyclic = self.graph.is_cyclic(title);
        let mut type_ref = TypeRef::new(to_rust_type_name(title));
        if is_cyclic {
          type_ref = type_ref.with_boxed();
        }
        return Ok(type_ref);
      }
    } else if let Some(ref title) = schema.title
      && self.graph.get_schema(title).is_some()
      && !schema.properties.is_empty()
    {
      // No explicit type, but has title matching a schema and has properties - likely a reference
      let is_cyclic = self.graph.is_cyclic(title);
      let mut type_ref = TypeRef::new(to_rust_type_name(title));
      if is_cyclic {
        type_ref = type_ref.with_boxed();
      }
      return Ok(type_ref);
    }

    // Check for inline oneOf/anyOf - detect nullable pattern
    if !schema.one_of.is_empty() || !schema.any_of.is_empty() {
      let variants = if !schema.one_of.is_empty() {
        &schema.one_of
      } else {
        &schema.any_of
      };

      // Check if this is the nullable pattern: anyOf/oneOf with [T, null]
      let has_null = variants.iter().any(|v| {
        if let Ok(resolved) = v.resolve(self.graph.spec()) {
          resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null))
        } else {
          false
        }
      });

      if has_null && variants.len() == 2 {
        // This is a nullable type - extract the non-null variant
        for variant_ref in variants {
          // Check if it's a direct $ref first
          if let ObjectOrReference::Ref { ref_path, .. } = variant_ref
            && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
          {
            return Ok(TypeRef::new(to_rust_type_name(&ref_name)).with_option());
          }

          // Otherwise resolve
          if let Ok(resolved) = variant_ref.resolve(self.graph.spec()) {
            // Skip null types
            if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
              continue;
            }

            // Found the actual type - recurse to get it
            let inner_type = self.schema_to_type_ref(&resolved)?;
            return Ok(inner_type.with_option());
          }
        }
      }

      // Try to extract type from the first non-null, non-string variant (for non-nullable unions)
      // Prefer complex types (arrays, objects) over simple types (strings)
      let mut fallback_type: Option<TypeRef> = None;

      for variant_ref in variants {
        // Check if it's a direct $ref
        if let ObjectOrReference::Ref { ref_path, .. } = variant_ref
          && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
        {
          return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
        }

        // Try resolving
        if let Ok(resolved) = variant_ref.resolve(self.graph.spec()) {
          // Skip null types
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Null)) {
            continue;
          }

          // Handle array types specially
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::Array)) {
            // Check array items for oneOf
            if let Some(ref items_box) = resolved.items
              && let Schema::Object(items_ref) = items_box.as_ref()
              && let Ok(items_schema) = items_ref.resolve(self.graph.spec())
            {
              // Items have oneOf - extract first ref
              if !items_schema.one_of.is_empty() {
                for one_of_ref in &items_schema.one_of {
                  if let ObjectOrReference::Ref { ref_path, .. } = one_of_ref
                    && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
                  {
                    return Ok(TypeRef::new(format!("Vec<{}>", to_rust_type_name(&ref_name))));
                  }
                }
              }
            }
          }

          // Save string types as fallback but prefer arrays/objects
          if resolved.schema_type == Some(SchemaTypeSet::Single(SchemaType::String)) && fallback_type.is_none() {
            fallback_type = Some(TypeRef::new("String"));
            continue;
          }

          // Check for nested oneOf (common pattern)
          if !resolved.one_of.is_empty() {
            for nested_ref in &resolved.one_of {
              if let ObjectOrReference::Ref { ref_path, .. } = nested_ref
                && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
              {
                return Ok(TypeRef::new(to_rust_type_name(&ref_name)));
              }
            }
          }

          // Use title if available
          if let Some(ref variant_title) = resolved.title
            && self.graph.get_schema(variant_title).is_some()
          {
            return Ok(TypeRef::new(to_rust_type_name(variant_title)));
          }
        }
      }

      // Use fallback if we found one
      if let Some(t) = fallback_type {
        return Ok(t);
      }

      // Fall through if we couldn't resolve to a concrete type
    }

    // Check schema type for primitives
    // This handles inline primitive types
    if let Some(ref schema_type) = schema.schema_type {
      match schema_type {
        SchemaTypeSet::Single(typ) => {
          let base_type = match typ {
            SchemaType::String => "String",
            SchemaType::Number => "f64",
            SchemaType::Integer => "i64",
            SchemaType::Boolean => "bool",
            SchemaType::Array => {
              // Handle array items
              if let Some(ref items_box) = schema.items
                && let Schema::Object(items_ref) = items_box.as_ref()
              {
                // Check if this is a $ref first
                if let ObjectOrReference::Ref { ref_path, .. } = items_ref.as_ref() {
                  // Extract the type name from the reference
                  if let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path) {
                    return Ok(TypeRef::new(format!("Vec<{}>", to_rust_type_name(&ref_name))));
                  }
                }

                // Otherwise resolve and check for oneOf/anyOf in items
                if let Ok(items_schema) = items_ref.resolve(self.graph.spec()) {
                  // If items have oneOf, extract the first ref type
                  if !items_schema.one_of.is_empty() {
                    for one_of_ref in &items_schema.one_of {
                      if let ObjectOrReference::Ref { ref_path, .. } = one_of_ref
                        && let Some(ref_name) = SchemaGraph::extract_ref_name(ref_path)
                      {
                        return Ok(TypeRef::new(format!("Vec<{}>", to_rust_type_name(&ref_name))));
                      }
                    }
                  }

                  // Regular item type conversion
                  let item_type = self.schema_to_type_ref(&items_schema)?;
                  return Ok(TypeRef::new(format!("Vec<{}>", item_type.to_rust_type())));
                }
              }
              return Ok(TypeRef::new("Vec<serde_json::Value>"));
            }
            SchemaType::Object => {
              // Object without a matching schema reference
              return Ok(TypeRef::new("serde_json::Value"));
            }
            SchemaType::Null => {
              return Ok(TypeRef::new("()").with_option());
            }
          };
          return Ok(TypeRef::new(base_type));
        }
        SchemaTypeSet::Multiple(_) => {
          // Handle nullable types - check if it's a simple nullable pattern
          return Ok(TypeRef::new("serde_json::Value"));
        }
      }
    }

    // Default to serde_json::Value for schemas without type or title
    Ok(TypeRef::new("serde_json::Value"))
  }
}

pub struct OperationConverter<'a> {
  schema_converter: &'a SchemaConverter<'a>,
  spec: &'a Spec,
}

impl<'a> OperationConverter<'a> {
  pub fn new(schema_converter: &'a SchemaConverter<'a>, spec: &'a Spec) -> Self {
    Self { schema_converter, spec }
  }

  /// Convert an operation to request and response types
  pub fn convert_operation(
    &self,
    _operation_id: &str,
    method: &str,
    path: &str,
    operation: &Operation,
  ) -> anyhow::Result<(Vec<RustType>, OperationInfo)> {
    let mut types = Vec::new();

    // Generate a base name for the operation
    let base_name = if let Some(ref op_id) = operation.operation_id {
      to_pascal_case(op_id)
    } else {
      // Fallback: use method + sanitized path
      let path_part = path.replace('/', "_").replace(['{', '}'], "");
      to_pascal_case(&format!("{}_{}", method, path_part))
    };

    // Generate request type if needed
    let request_type_name = if !operation.parameters.is_empty() || operation.request_body.is_some() {
      let request_name = format!("{}Request", base_name);
      let request_struct = self.create_request_struct(&request_name, operation)?;
      types.push(RustType::Struct(request_struct));
      Some(request_name)
    } else {
      None
    };

    // Extract primary response type (typically 200/201 response)
    // Don't generate response enums - let HTTP clients use http::StatusCode
    let response_type_name = if let Some(ref responses) = operation.responses {
      // Look for successful response (200, 201, etc.)
      responses
        .iter()
        .find(|(code, _)| code.starts_with('2'))
        .or_else(|| responses.iter().next())
        .and_then(|(_, response_ref)| {
          if let Ok(response) = response_ref.resolve(self.spec) {
            self.extract_response_schema_name(&response)
          } else {
            None
          }
        })
        .map(|name| to_rust_type_name(&name))
    } else {
      None
    };

    let op_info = OperationInfo {
      operation_id: operation.operation_id.clone().unwrap_or_else(|| base_name.clone()),
      method: method.to_string(),
      path: path.to_string(),
      summary: operation.summary.clone(),
      description: operation.description.clone(),
      request_type: request_type_name,
      response_type: response_type_name,
    };

    Ok((types, op_info))
  }

  /// Create a request struct from operation parameters and body
  fn create_request_struct(&self, name: &str, operation: &Operation) -> anyhow::Result<StructDef> {
    let mut fields = Vec::new();

    // Process parameters
    let mut params: Vec<_> = operation
      .parameters
      .iter()
      .filter_map(|param_ref| param_ref.resolve(self.spec).ok())
      .collect();

    params.sort_by(|a, b| {
      let rank = |loc: &ParameterIn| match loc {
        ParameterIn::Path => 0u8,
        ParameterIn::Query => 1,
        ParameterIn::Header => 2,
        ParameterIn::Cookie => 3,
      };

      match rank(&a.location).cmp(&rank(&b.location)) {
        Ordering::Equal => a.name.cmp(&b.name),
        other => other,
      }
    });

    for param in params {
      let field = self.convert_parameter(&param)?;
      fields.push(field);
    }

    // Process request body
    if let Some(ref body_ref) = operation.request_body
      && let Ok(body) = body_ref.resolve(self.spec)
    {
      // Extract schema from the first content type (usually application/json)
      if let Some((_content_type, media_type)) = body.content.iter().next()
        && let Some(ref schema_ref) = media_type.schema
        && let Ok(schema) = schema_ref.resolve(self.spec)
      {
        let body_type = self.schema_converter.schema_to_type_ref(&schema)?;
        let is_required = body.required.unwrap_or(false);
        let validation_attrs = self
          .schema_converter
          .extract_validation_attrs(name, is_required, &schema);
        let regex_validation = self.schema_converter.extract_validation_pattern(name, &schema).cloned();
        let default_value = self.schema_converter.extract_default_value(&schema);

        let mut serde_attrs = vec![];
        if !is_required {
          serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
        }

        fields.push(FieldDef {
          name: "body".to_string(),
          docs: body
            .description
            .as_ref()
            .map(|d| doc_comment_lines(d))
            .unwrap_or_default(),
          rust_type: if is_required {
            body_type
          } else {
            body_type.with_option()
          },
          optional: !is_required,
          serde_attrs,
          validation_attrs,
          regex_validation,
          default_value,
        });
      }
    }

    let docs = operation
      .description
      .as_ref()
      .or(operation.summary.as_ref())
      .map(|d| doc_comment_lines(d))
      .unwrap_or_default();

    // Detect consistent naming pattern for rename_all
    let mut serde_attrs = vec![];
    let renamed_fields: Vec<_> = fields
      .iter()
      .filter_map(|f| {
        f.serde_attrs
          .iter()
          .find(|attr| attr.starts_with("rename = "))
          .map(|attr| {
            let start = attr.find('"').unwrap() + 1;
            let end = attr.rfind('"').unwrap();
            attr[start..end].to_string()
          })
      })
      .collect();

    if !renamed_fields.is_empty() {
      let all_kebab = renamed_fields.iter().all(|name| name.contains('-'));
      if all_kebab {
        serde_attrs.push("rename_all = \"kebab-case\"".to_string());
        // Remove individual rename attributes
        for field in &mut fields {
          field.serde_attrs.retain(|attr| !attr.starts_with("rename = "));
        }
      }
    }

    // Only add serde(default) at struct level if ALL fields have defaults or are Option/Vec
    let all_fields_defaultable = fields.iter().all(|f| {
      f.default_value.is_some()
        || f.rust_type.nullable
        || f.rust_type.is_array
        || matches!(
          f.rust_type.base_type.as_str(),
          "String"
            | "bool"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
            | "serde_json::Value"
        )
    });

    if all_fields_defaultable && fields.iter().any(|f| f.default_value.is_some()) {
      serde_attrs.push("default".to_string());
    }

    Ok(StructDef {
      name: to_rust_type_name(name),
      docs,
      fields,
      derives: vec![
        "Debug".into(),
        "Clone".into(),
        "Serialize".into(),
        "Deserialize".into(),
        "Validate".into(),
      ],
      serde_attrs,
    })
  }

  /// Convert a parameter to a field definition
  fn convert_parameter(&self, param: &Parameter) -> anyhow::Result<FieldDef> {
    let (rust_type, validation_attrs, regex_validation, default_value) = if let Some(ref schema_ref) = param.schema {
      if let Ok(schema) = schema_ref.resolve(self.spec) {
        let type_ref = self.schema_converter.schema_to_type_ref(&schema)?;
        let is_required = param.required.unwrap_or(false);
        let validation = self
          .schema_converter
          .extract_validation_attrs(&param.name, is_required, &schema);
        let regex_validation = self.schema_converter.extract_validation_pattern(&param.name, &schema);
        let default = self.schema_converter.extract_default_value(&schema);
        (type_ref, validation, regex_validation.cloned(), default)
      } else {
        (TypeRef::new("String"), vec![], None, None)
      }
    } else {
      (TypeRef::new("String"), vec![], None, None)
    };

    let is_required = param.required.unwrap_or(false);

    let mut serde_attrs = vec![];
    // Add rename if the parameter name is not a valid Rust identifier
    if param.name.contains('-') || param.name.contains('.') {
      serde_attrs.push(format!("rename = \"{}\"", param.name));
    }

    // Add skip_serializing_if for optional parameters
    if !is_required {
      serde_attrs.push("skip_serializing_if = \"Option::is_none\"".to_string());
    }

    // Add location hint as a comment in docs
    let location_hint = match param.location {
      ParameterIn::Path => "Path parameter",
      ParameterIn::Query => "Query parameter",
      ParameterIn::Header => "Header parameter",
      ParameterIn::Cookie => "Cookie parameter",
    };

    let mut docs = vec![format!("/// {}", location_hint)];
    if let Some(ref desc) = param.description {
      docs.extend(doc_comment_lines(desc));
    }

    Ok(FieldDef {
      name: to_rust_ident(&param.name),
      docs,
      rust_type: if is_required {
        rust_type
      } else {
        rust_type.with_option()
      },
      optional: !is_required,
      serde_attrs,
      validation_attrs,
      regex_validation,
      default_value,
    })
  }

  /// Extract schema name from a response (helper)
  fn extract_response_schema_name(&self, response: &oas3::spec::Response) -> Option<String> {
    response.content.iter().next().and_then(|(_, media_type)| {
      media_type.schema.as_ref().and_then(|schema_ref| {
        if let ObjectOrReference::Ref { ref_path, .. } = schema_ref {
          SchemaGraph::extract_ref_name(ref_path)
        } else {
          None
        }
      })
    })
  }
}

pub struct CodeGenerator;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RegexKey {
  type_name: String,
  variant_name: Option<String>,
  field_name: String,
}

impl RegexKey {
  fn for_struct(type_name: &str, field_name: &str) -> Self {
    Self {
      type_name: type_name.to_string(),
      variant_name: None,
      field_name: field_name.to_string(),
    }
  }

  fn for_variant(type_name: &str, variant_name: &str, field_name: &str) -> Self {
    Self {
      type_name: type_name.to_string(),
      variant_name: Some(variant_name.to_string()),
      field_name: field_name.to_string(),
    }
  }

  fn parts(&self) -> Vec<&str> {
    let mut parts = vec![self.type_name.as_str()];
    if let Some(variant) = &self.variant_name {
      parts.push(variant.as_str());
    }
    parts.push(self.field_name.as_str());
    parts
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TypeKind {
  Struct,
  Enum,
  Alias,
}

impl CodeGenerator {
  pub fn generate(types: &[RustType]) -> TokenStream {
    let ordered = Self::ordered_types(types);
    let (regex_consts, regex_lookup) = Self::generate_regex_constants(&ordered);
    let type_tokens: Vec<TokenStream> = ordered
      .iter()
      .map(|ty| Self::generate_type(ty, &regex_lookup))
      .collect();
    let default_impls = Self::generate_default_impls(&ordered);

    quote! {
      use serde::{Deserialize, Serialize};
      use validator::Validate;

      #regex_consts

      #(#type_tokens)*

      #default_impls
    }
  }

  fn ordered_types<'a>(types: &'a [RustType]) -> Vec<&'a RustType> {
    let mut map: BTreeMap<(TypeKind, String), &'a RustType> = BTreeMap::new();
    for ty in types {
      let key = Self::type_key(ty);
      map.entry(key).or_insert(ty);
    }
    map.into_values().collect()
  }

  fn type_key(rust_type: &RustType) -> (TypeKind, String) {
    let kind = match rust_type {
      RustType::Struct(_) => TypeKind::Struct,
      RustType::Enum(_) => TypeKind::Enum,
      RustType::TypeAlias(_) => TypeKind::Alias,
    };

    (kind, rust_type.type_name().to_string())
  }

  /// Generate regex constants for validation
  fn generate_regex_constants(types: &[&RustType]) -> (TokenStream, BTreeMap<RegexKey, String>) {
    let mut const_defs: BTreeMap<String, String> = BTreeMap::new();
    let mut lookup: BTreeMap<RegexKey, String> = BTreeMap::new();
    let mut pattern_to_const: BTreeMap<String, String> = BTreeMap::new();

    for rust_type in types {
      match rust_type {
        RustType::Struct(def) => {
          for field in &def.fields {
            let Some(pattern) = &field.regex_validation else {
              continue;
            };
            let key = RegexKey::for_struct(&def.name, &field.name);
            let pattern_key = pattern.clone();
            let const_name = match pattern_to_const.get(&pattern_key) {
              Some(existing) => existing.clone(),
              None => {
                let name = Self::regex_const_name(&key);
                pattern_to_const.insert(pattern_key.clone(), name.clone());
                const_defs.insert(name.clone(), pattern_key);
                name
              }
            };
            lookup.insert(key, const_name);
          }
        }
        RustType::Enum(def) => {
          for variant in &def.variants {
            if let VariantContent::Struct(fields) = &variant.content {
              for field in fields {
                let Some(pattern) = &field.regex_validation else {
                  continue;
                };
                let key = RegexKey::for_variant(&def.name, &variant.name, &field.name);
                let pattern_key = pattern.clone();
                let const_name = match pattern_to_const.get(&pattern_key) {
                  Some(existing) => existing.clone(),
                  None => {
                    let name = Self::regex_const_name(&key);
                    pattern_to_const.insert(pattern_key.clone(), name.clone());
                    const_defs.insert(name.clone(), pattern_key);
                    name
                  }
                };
                lookup.insert(key, const_name);
              }
            }
          }
        }
        RustType::TypeAlias(_) => {}
      }
    }

    if const_defs.is_empty() {
      return (quote! {}, lookup);
    }

    let regex_defs: Vec<TokenStream> = const_defs
      .into_iter()
      .map(|(name, pattern)| {
        let ident = format_ident!("{}", name);
        quote! {
          static #ident: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(#pattern).expect("invalid regex"));
        }
      })
      .collect();

    (quote! { #(#regex_defs)* }, lookup)
  }

  fn regex_const_name(key: &RegexKey) -> String {
    let joined = key
      .parts()
      .into_iter()
      .map(any_ascii::any_ascii)
      .collect::<Vec<_>>()
      .join("_");

    format!("REGEX_{}", joined.to_constant_case())
  }

  /// Check if a type can safely use Default::default()
  fn type_can_default(type_ref: &TypeRef) -> bool {
    let base_type = &type_ref.base_type;

    // Primitive types that implement Default
    matches!(
      base_type.as_str(),
      "String"
        | "bool"
        | "i8"
        | "i16"
        | "i32"
        | "i64"
        | "i128"
        | "isize"
        | "u8"
        | "u16"
        | "u32"
        | "u64"
        | "u128"
        | "usize"
        | "f32"
        | "f64"
        | "serde_json::Value"
    ) || type_ref.nullable
      || type_ref.is_array
      || base_type.starts_with("Vec<")
      || base_type.starts_with("Option<")
  }

  /// Find the best variant to use as default for an enum
  /// Priority: Unit > Vec/Option types > Struct variants > Tuple variants with primitives
  fn find_best_default_variant(variants: &[VariantDef]) -> Option<(&VariantDef, TokenStream)> {
    // Priority 1: Unit variants
    for variant in variants {
      if matches!(variant.content, VariantContent::Unit) {
        let variant_name = format_ident!("{}", variant.name);
        return Some((variant, quote! { Self::#variant_name }));
      }
    }

    // Priority 2: Tuple variants with Vec or Option (always defaultable)
    for variant in variants {
      if let VariantContent::Tuple(types) = &variant.content
        && types.len() == 1
        && (types[0].is_array || types[0].nullable)
      {
        let variant_name = format_ident!("{}", variant.name);
        return Some((variant, quote! { Self::#variant_name(Default::default()) }));
      }
    }

    // Priority 3: Struct variants with defaultable fields
    for variant in variants {
      if let VariantContent::Struct(fields) = &variant.content {
        // Check if all fields can be defaulted
        let all_defaultable = fields
          .iter()
          .all(|f| f.default_value.is_some() || Self::type_can_default(&f.rust_type));

        if all_defaultable {
          let variant_name = format_ident!("{}", variant.name);
          let field_inits: Vec<TokenStream> = fields
            .iter()
            .map(|field| {
              let field_name = format_ident!("{}", field.name);
              if let Some(ref default_val) = field.default_value {
                let value_expr = Self::json_value_to_rust_expr(default_val, &field.rust_type);
                quote! { #field_name: #value_expr }
              } else {
                quote! { #field_name: Default::default() }
              }
            })
            .collect();
          return Some((variant, quote! { Self::#variant_name { #(#field_inits),* } }));
        }
      }
    }

    // Priority 4: Tuple variants with primitive types
    for variant in variants {
      if let VariantContent::Tuple(types) = &variant.content
        && types.iter().all(Self::type_can_default)
      {
        let variant_name = format_ident!("{}", variant.name);
        let defaults: Vec<TokenStream> = types.iter().map(|_| quote! { Default::default() }).collect();
        return Some((variant, quote! { Self::#variant_name(#(#defaults),*) }));
      }
    }

    // No suitable variant found
    None
  }

  /// Generate impl Default blocks for structs and enums
  fn generate_default_impls(types: &[&RustType]) -> TokenStream {
    let mut impls: Vec<(proc_macro2::Ident, TokenStream)> = Vec::new();

    for rust_type in types {
      match rust_type {
        RustType::Struct(def) => {
          let has_defaults = def.fields.iter().any(|f| f.default_value.is_some());

          if has_defaults {
            let all_fields_can_default = def
              .fields
              .iter()
              .all(|f| f.default_value.is_some() || Self::type_can_default(&f.rust_type));

            if all_fields_can_default {
              let struct_name = format_ident!("{}", def.name);

              let field_inits: Vec<TokenStream> = def
                .fields
                .iter()
                .map(|field| {
                  let field_name = format_ident!("{}", field.name);

                  if let Some(ref default_val) = field.default_value {
                    let value_expr = Self::json_value_to_rust_expr(default_val, &field.rust_type);
                    quote! { #field_name: #value_expr }
                  } else {
                    quote! { #field_name: Default::default() }
                  }
                })
                .collect();

              impls.push((struct_name, quote! { Self { #(#field_inits),* } }));
            }
          }
        }
        RustType::Enum(def) => {
          if let Some((_variant, default_expr)) = Self::find_best_default_variant(&def.variants) {
            let enum_name = format_ident!("{}", def.name);
            impls.push((enum_name, default_expr));
          }
        }
        RustType::TypeAlias(_) => {}
      }
    }

    if impls.is_empty() {
      return quote! {};
    }

    let macro_def = quote! {
      macro_rules! impl_default {
        ($type:ident = $body:expr) => {
          impl Default for $type {
            fn default() -> Self {
              $body
            }
          }
        };
      }
    };

    let macro_calls: Vec<TokenStream> = impls
      .into_iter()
      .map(|(ident, body)| quote! { impl_default!(#ident = #body); })
      .collect();

    quote! {
      #macro_def

      #(#macro_calls)*
    }
  }

  /// Convert a JSON value to a Rust expression
  fn json_value_to_rust_expr(value: &serde_json::Value, rust_type: &TypeRef) -> TokenStream {
    let base_expr = match value {
      serde_json::Value::String(s) => {
        quote! { #s.to_string() }
      }
      serde_json::Value::Number(n) => {
        if let Some(i) = n.as_i64() {
          quote! { #i }
        } else if let Some(f) = n.as_f64() {
          quote! { #f }
        } else {
          quote! { Default::default() }
        }
      }
      serde_json::Value::Bool(b) => {
        quote! { #b }
      }
      serde_json::Value::Null => {
        quote! { None }
      }
      serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
        // For complex types, use Default::default()
        quote! { Default::default() }
      }
    };

    // Wrap in Some() if the type is Option<T>
    if rust_type.nullable && !matches!(value, serde_json::Value::Null) {
      quote! { Some(#base_expr) }
    } else {
      base_expr
    }
  }

  fn generate_type(rust_type: &RustType, regex_lookup: &BTreeMap<RegexKey, String>) -> TokenStream {
    match rust_type {
      RustType::Struct(def) => Self::generate_struct(def, regex_lookup),
      RustType::Enum(def) => Self::generate_enum(def, regex_lookup),
      RustType::TypeAlias(def) => Self::generate_type_alias(def),
    }
  }

  fn generate_struct(def: &StructDef, regex_lookup: &BTreeMap<RegexKey, String>) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let derives = Self::generate_derives(&def.derives);
    let serde_attrs = Self::generate_serde_attrs(&def.serde_attrs);
    let fields = Self::generate_fields_with_visibility(&def.name, None, &def.fields, true, true, regex_lookup);

    quote! {
      #docs
      #derives
      #serde_attrs
      pub struct #name {
        #(#fields),*
      }
    }
  }

  fn generate_enum(def: &EnumDef, regex_lookup: &BTreeMap<RegexKey, String>) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let derives = Self::generate_derives(&def.derives);
    let serde_attrs = Self::generate_enum_serde_attrs(def);
    let variants = Self::generate_variants(&def.name, &def.variants, regex_lookup);

    quote! {
      #docs
      #derives
      #serde_attrs
      pub enum #name {
        #(#variants),*
      }
    }
  }

  fn generate_type_alias(def: &TypeAliasDef) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let target = Self::parse_type_string(&def.target.to_rust_type());

    quote! {
      #docs
      pub type #name = #target;
    }
  }

  fn generate_docs(docs: &[String]) -> TokenStream {
    if docs.is_empty() {
      return quote! {};
    }

    let doc_lines: Vec<TokenStream> = docs
      .iter()
      .map(|line| {
        // Remove the /// prefix that was added earlier
        let clean_line = line.strip_prefix("/// ").unwrap_or(line);
        quote! { #[doc = #clean_line] }
      })
      .collect();

    quote! { #(#doc_lines)* }
  }

  fn generate_derives(derives: &[String]) -> TokenStream {
    if derives.is_empty() {
      return quote! {};
    }

    let derive_idents: Vec<_> = derives.iter().map(|d| format_ident!("{}", d)).collect();

    quote! {
      #[derive(#(#derive_idents),*)]
    }
  }

  fn generate_serde_attrs(attrs: &[String]) -> TokenStream {
    if attrs.is_empty() {
      return quote! {};
    }

    let attr_tokens: Vec<TokenStream> = attrs
      .iter()
      .map(|attr| {
        let attr_str = attr.as_str();
        let tokens: TokenStream = attr_str.parse().unwrap_or_else(|_| quote! {});
        quote! { #[serde(#tokens)] }
      })
      .collect();

    quote! { #(#attr_tokens)* }
  }

  fn generate_validation_attrs(regex_const: Option<&str>, attrs: &[String]) -> TokenStream {
    if attrs.is_empty() && regex_const.is_none() {
      return quote! {};
    }

    let mut combined = attrs.to_owned();

    if let Some(const_name) = regex_const {
      combined.push(format!("regex(path = \"{}\")", const_name));
    }

    let attr_tokens: Vec<TokenStream> = combined
      .iter()
      .map(|attr| attr.parse().unwrap_or_else(|_| quote! {}))
      .collect();

    quote! { #[validate(#(#attr_tokens),*)] }
  }

  fn generate_enum_serde_attrs(def: &EnumDef) -> TokenStream {
    let mut attrs = Vec::new();

    // Add discriminator tag if present
    if let Some(ref discriminator) = def.discriminator {
      attrs.push(quote! { tag = #discriminator });
    }

    // Add other serde attributes
    for attr in &def.serde_attrs {
      if let Ok(tokens) = attr.parse::<TokenStream>() {
        attrs.push(tokens);
      }
    }

    if attrs.is_empty() {
      return quote! {};
    }

    quote! {
      #[serde(#(#attrs),*)]
    }
  }

  fn generate_fields_with_visibility(
    type_name: &str,
    variant_name: Option<&str>,
    fields: &[FieldDef],
    add_pub: bool,
    include_validation: bool,
    regex_lookup: &BTreeMap<RegexKey, String>,
  ) -> Vec<TokenStream> {
    fields
      .iter()
      .map(|field| {
        let name = format_ident!("{}", field.name);
        let docs = Self::generate_docs(&field.docs);
        let serde_attrs = Self::generate_serde_attrs(&field.serde_attrs);

        // Only include validation for struct fields, not enum variant fields
        let regex_const = if include_validation && field.regex_validation.is_some() {
          let key = match variant_name {
            Some(variant) => RegexKey::for_variant(type_name, variant, &field.name),
            None => RegexKey::for_struct(type_name, &field.name),
          };
          regex_lookup.get(&key).map(|s| s.as_str())
        } else {
          None
        };

        let validation_attrs = if include_validation {
          Self::generate_validation_attrs(regex_const, &field.validation_attrs)
        } else {
          quote! {}
        };
        let type_tokens = Self::parse_type_string(&field.rust_type.to_rust_type());

        if add_pub {
          quote! {
            #docs
            #serde_attrs
            #validation_attrs
            pub #name: #type_tokens
          }
        } else {
          quote! {
            #docs
            #serde_attrs
            #validation_attrs
            #name: #type_tokens
          }
        }
      })
      .collect()
  }

  fn generate_variants(
    type_name: &str,
    variants: &[VariantDef],
    regex_lookup: &BTreeMap<RegexKey, String>,
  ) -> Vec<TokenStream> {
    variants
      .iter()
      .map(|variant| {
        let name = format_ident!("{}", variant.name);
        let docs = Self::generate_docs(&variant.docs);
        let serde_attrs = Self::generate_serde_attrs(&variant.serde_attrs);

        let content = match &variant.content {
          VariantContent::Unit => quote! {},
          VariantContent::Tuple(types) => {
            let type_tokens: Vec<_> = types
              .iter()
              .map(|t| Self::parse_type_string(&t.to_rust_type()))
              .collect();
            quote! { ( #(#type_tokens),* ) }
          }
          VariantContent::Struct(fields) => {
            // Enum variant fields should not have 'pub' keyword or validation attributes
            let field_tokens =
              Self::generate_fields_with_visibility(type_name, Some(&variant.name), fields, false, false, regex_lookup);
            quote! { { #(#field_tokens),* } }
          }
        };

        quote! {
          #docs
          #serde_attrs
          #name #content
        }
      })
      .collect()
  }

  /// Parse a type string into a TokenStream
  /// This is a simple parser that handles basic Rust types
  fn parse_type_string(type_str: &str) -> TokenStream {
    // For now, just parse it directly - this works for most cases
    type_str.parse().unwrap_or_else(|_| quote! { serde_json::Value })
  }
}
