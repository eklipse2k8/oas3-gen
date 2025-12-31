use oas3::spec::ObjectSchema;

use super::{
  CodegenConfig,
  cache::SharedSchemaCache,
  union::{CollisionStrategy, EnumValueEntry, UnionConverter},
};
use crate::generator::ast::{Documentation, RustType};

#[derive(Clone, Debug)]
pub(crate) struct EnumConverter {
  preserve_case_variants: bool,
  case_insensitive_enums: bool,
}

impl EnumConverter {
  pub(crate) fn new(config: &CodegenConfig) -> Self {
    Self {
      preserve_case_variants: config.preserve_case_variants(),
      case_insensitive_enums: config.case_insensitive_enums(),
    }
  }

  pub(crate) fn convert_value_enum(
    &self,
    name: &str,
    schema: &ObjectSchema,
    cache: Option<&mut SharedSchemaCache>,
  ) -> RustType {
    let strategy = if self.preserve_case_variants {
      CollisionStrategy::Preserve
    } else {
      CollisionStrategy::Deduplicate
    };

    let entries: Vec<EnumValueEntry> = schema
      .enum_values
      .iter()
      .cloned()
      .map(|value| EnumValueEntry {
        value,
        docs: Documentation::default(),
        deprecated: false,
      })
      .collect();

    let enum_def = UnionConverter::build_enum_from_values(
      name,
      &entries,
      strategy,
      Documentation::from_optional(schema.description.as_ref()),
      self.case_insensitive_enums,
    );

    if let (Some(c), RustType::Enum(e)) = (cache, &enum_def) {
      c.mark_name_used(e.name.to_string());
    }

    enum_def
  }
}
