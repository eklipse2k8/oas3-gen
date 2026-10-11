macro_rules! config {
  ($($field:ident: $value:expr),* $(,)?) => {
    oas3_gen::generator::CodegenConfig {
      $($field: $value,)*
      schema_scope: oas3_gen::generator::SchemaScope::All,
      ..oas3_gen::generator::CodegenConfig::default()
    }
  };
}

mod cache;
mod discriminator;
mod fields;
mod inline_resolver;
mod methods;
mod parameters;
mod requests;
mod responses;
mod type_resolver;
mod unions;
mod value_enums;

use std::{rc::Rc, sync::Arc};

use oas3_gen::generator::{
  CodegenConfig,
  ast::{
    DiscriminatedEnumDef, EnumDef, FieldDef, OperationInfo, RustType, StructDef, TypeRef, VariantContent, VariantDef,
  },
  converter::{ConverterContext, OperationsProcessor, SchemaConverter, cache::SharedSchemaCache},
  metrics::GenerationStats,
  operation_registry::OperationRegistry,
  schema_registry::SchemaRegistry,
};

struct Converted {
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  warnings: Vec<String>,
}

impl Converted {
  #[track_caller]
  fn enum_def(&self, name: &str) -> &EnumDef {
    self
      .types
      .iter()
      .find_map(|rust_type| match rust_type {
        RustType::Enum(def) if def.name == name => Some(def),
        _ => None,
      })
      .unwrap_or_else(|| panic!("enum {name} missing from {:?}", self.declarations()))
  }

  #[track_caller]
  fn struct_def(&self, name: &str) -> &StructDef {
    self
      .types
      .iter()
      .find_map(|rust_type| match rust_type {
        RustType::Struct(def) if def.name == name => Some(def),
        _ => None,
      })
      .unwrap_or_else(|| panic!("struct {name} missing from {:?}", self.declarations()))
  }

  #[track_caller]
  fn discriminated_enum(&self, name: &str) -> &DiscriminatedEnumDef {
    self
      .types
      .iter()
      .find_map(|rust_type| match rust_type {
        RustType::DiscriminatedEnum(def) if def.name == name => Some(def),
        _ => None,
      })
      .unwrap_or_else(|| panic!("discriminated enum {name} missing from {:?}", self.declarations()))
  }

  #[track_caller]
  fn operation(&self, stable_id: &str) -> &OperationInfo {
    self
      .operations
      .iter()
      .find(|operation| operation.stable_id == stable_id)
      .unwrap_or_else(|| panic!("operation {stable_id} missing"))
  }

  fn fields(&self, owner: &str) -> Vec<String> {
    self
      .struct_def(owner)
      .fields
      .iter()
      .map(|field| format!("{}: {}", field.name, field.rust_type.to_rust_type()))
      .collect()
  }

  fn declarations(&self) -> Vec<String> {
    self
      .types
      .iter()
      .map(|rust_type| match rust_type {
        RustType::Struct(def) => format!("struct {}", def.name),
        RustType::TypeAlias(def) => format!("type {} = {}", def.name, def.target.to_rust_type()),
        other => format!("enum {}", other.type_name()),
      })
      .collect()
  }
}

#[track_caller]
fn field<'a>(def: &'a StructDef, name: &str) -> &'a FieldDef {
  def
    .fields
    .iter()
    .find(|field| field.name == name)
    .unwrap_or_else(|| panic!("{} has no field {name}", def.name))
}

fn variant_signature(variant: &VariantDef) -> String {
  match &variant.content {
    VariantContent::Unit => variant.name.to_string(),
    VariantContent::Tuple(types) => format!(
      "{}({})",
      variant.name,
      types.iter().map(TypeRef::to_rust_type).collect::<Vec<_>>().join(", ")
    ),
  }
}

fn enum_signature(def: &EnumDef) -> String {
  let variants = def
    .variants
    .iter()
    .map(|variant| {
      let signature = variant_signature(variant);
      if variant.default {
        format!("#[default] {signature}")
      } else {
        signature
      }
    })
    .collect::<Vec<_>>();
  format!("{} {{ {} }}", def.name, variants.join(", "))
}

#[track_caller]
fn convert(spec_json: &str, config: CodegenConfig) -> Converted {
  let spec = crate::parse_spec(spec_json);
  let mut stats = GenerationStats::default();
  let mut registry = SchemaRegistry::new(&spec, &mut stats);
  let mut cache = SharedSchemaCache::new();
  cache.initialize_from_schemas(registry.schemas());
  let fingerprints = cache.union_fingerprints().clone();
  let operation_registry = OperationRegistry::with_filters(&spec, None, None, &config.fn_name_overrides);
  let (_, reachable) = registry.initialize(&operation_registry, config.include_all_schemas(), &fingerprints);
  let registry = Arc::new(registry);
  let names = registry.scan_and_compute_names().expect("schema names compute");
  cache.set_precomputed_names(names.names, names.enum_names, names.schema_metadata);
  let reachable = reachable.map(Arc::new);
  let context = Rc::new(ConverterContext::new(registry, config, cache, reachable.clone()));
  let converter = SchemaConverter::new(&context);
  let mut types = converter.convert_all_schemas(reachable.as_deref(), &mut stats);
  let output = OperationsProcessor::new(context.clone(), &converter).process_all(operation_registry.operations());
  types.extend(output.types);
  types.extend(context.cache_mut().take_types());
  stats.record_warnings(output.warnings);
  Converted {
    types,
    operations: output.operations,
    warnings: stats.warnings.iter().map(ToString::to_string).collect(),
  }
}
