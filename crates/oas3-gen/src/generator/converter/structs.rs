use std::{collections::BTreeSet, rc::Rc};

use oas3::spec::{ObjectSchema, Schema};

use super::{ConversionOutput, discriminator::DiscriminatorConverter, fields::FieldConverter};
use crate::{
  generator::{
    ast::{
      DeriveTrait, Documentation, FieldCollection as _, RustType, SerdeAttribute, StructDef, StructKind, StructToken,
    },
    converter::ConverterContext,
    naming::{constants::DISCRIMINATED_BASE_SUFFIX, identifiers::to_rust_type_name},
  },
  utils::SchemaExt,
};

#[derive(Clone, Debug)]
pub(crate) struct StructConverter {
  context: Rc<ConverterContext>,
  field_converter: FieldConverter,
  discriminator_converter: DiscriminatorConverter,
}

impl StructConverter {
  /// Creates a new struct converter with field and discriminator sub-converters.
  ///
  /// The `context` provides access to the schema registry, configuration,
  /// and shared type cache for deduplication.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let field_converter = FieldConverter::new(&context);
    let discriminator_converter = DiscriminatorConverter::new(context.clone());
    Self {
      context,
      field_converter,
      discriminator_converter,
    }
  }

  /// Derives the Rust struct name from an OpenAPI schema name.
  ///
  /// For schemas that define a discriminator (polymorphic base types), appends
  /// a `Base` suffix to distinguish the struct from the generated discriminated
  /// enum. Otherwise, converts the name to PascalCase.
  pub(crate) fn struct_name(name: &str, schema: &ObjectSchema) -> StructToken {
    if schema.is_discriminated_base_type() {
      StructToken::from(format!("{}{}", to_rust_type_name(name), DISCRIMINATED_BASE_SUFFIX))
    } else {
      StructToken::from_raw(name)
    }
  }

  /// Assembles a struct definition from schema properties.
  ///
  /// Collects fields via [`FieldConverter::build_struct_fields`], then derives
  /// struct-level serde attributes, outer attributes, and builder configuration
  /// from the resulting field set. Returns the struct definition along with any
  /// inline types extracted from nested object or enum properties.
  fn build_struct(
    &self,
    name: StructToken,
    schema: &ObjectSchema,
    schema_name: Option<&str>,
    kind: StructKind,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let field_result = self
      .field_converter
      .build_struct_fields(name.as_str(), schema, schema_name, kind)?;

    let fields = field_result.result;

    let deny_unknown = matches!(&schema.additional_properties, Some(Schema::Boolean(b)) if !b.0);
    let serde_attrs = fields.struct_serde_attrs(
      deny_unknown
        .then_some(SerdeAttribute::DenyUnknownFields)
        .into_iter()
        .collect(),
    );

    let enable_builders = matches!(kind, StructKind::Schema) && self.context.config().enable_builders();
    let additional_derives = if enable_builders {
      BTreeSet::from([DeriveTrait::Builder])
    } else {
      BTreeSet::default()
    };

    let struct_def = StructDef::builder()
      .name(name)
      .docs(Documentation::from_optional(schema.description.as_ref()))
      .outer_attrs(fields.struct_outer_attrs())
      .serde_attrs(serde_attrs)
      .fields(fields)
      .kind(kind)
      .additional_derives(additional_derives)
      .build();

    Ok(ConversionOutput::with_inline_types(
      RustType::Struct(struct_def),
      field_result.inline_types,
    ))
  }

  /// Converts a child schema in a discriminated union hierarchy.
  ///
  /// For schemas that inherit from a discriminated base type via `allOf`,
  /// this creates a struct with all merged fields. The parent must have
  /// a `discriminator` definition; otherwise, an error is returned.
  fn convert_discriminated_child(
    &self,
    name: &str,
    merged_schema: &ObjectSchema,
    parent_schema: &ObjectSchema,
  ) -> anyhow::Result<Vec<RustType>> {
    if parent_schema.discriminator.is_none() {
      anyhow::bail!("Parent schema for discriminated child '{name}' is not a valid discriminator base");
    }

    let result = self.build_struct(
      StructToken::from_raw(name),
      merged_schema,
      Some(name),
      StructKind::Schema,
    )?;

    Ok(std::iter::once(result.result).chain(result.inline_types).collect())
  }

  /// Converts an OpenAPI object schema into a Rust struct definition.
  ///
  /// Routes to [`build_struct`] for field extraction and registers the
  /// result in the shared cache for deduplication. The `kind` parameter
  /// controls struct decoration (e.g., derive macros, serde attributes).
  pub(crate) fn convert_struct(
    &self,
    name: &str,
    schema: &ObjectSchema,
    kind: Option<StructKind>,
  ) -> anyhow::Result<ConversionOutput<RustType>> {
    let struct_name = Self::struct_name(name, schema);

    let result = self.build_struct(
      struct_name.clone(),
      schema,
      Some(name),
      kind.unwrap_or(StructKind::Schema),
    )?;

    self.context.cache.borrow_mut().register_struct_def(
      struct_name.as_str(),
      match &result.result {
        RustType::Struct(def) => def.clone(),
        _ => unreachable!(),
      },
    );

    Ok(result)
  }

  /// Converts an `allOf` schema by merging all constituent schemas into one struct.
  ///
  /// If the schema participates in a discriminated union (has a parent with
  /// a `discriminator`), delegates to [`convert_discriminated_child`].
  /// Otherwise, merges all properties and builds a standard struct.
  pub(crate) fn convert_all_of_schema(&self, name: &str) -> anyhow::Result<Vec<RustType>> {
    let graph = self.context.graph();

    let merged_info = graph
      .merged(name)
      .ok_or_else(|| anyhow::anyhow!("Schema '{name}' not found in registry"))?;

    if let Some(parent_name) = self.discriminator_converter.detect_discriminated_parent(name) {
      let parent_merged = graph
        .merged(parent_name)
        .ok_or_else(|| anyhow::anyhow!("Parent schema '{parent_name}' not found"))?;
      return self.convert_discriminated_child(name, &merged_info.schema, &parent_merged.schema);
    }

    let effective_schema = graph.resolved(name).unwrap_or(&merged_info.schema);

    let result = self.build_struct(
      Self::struct_name(name, effective_schema),
      effective_schema,
      Some(name),
      StructKind::Schema,
    )?;
    self.finalize_struct_types(name, effective_schema, result.result, result.inline_types)
  }

  /// Assembles the final type collection, optionally prepending a discriminated enum.
  ///
  /// For schemas that define a `discriminator`, creates a tagged union enum
  /// that wraps the base struct and all subtypes. The discriminated enum
  /// is placed first in the output vector, followed by the struct and
  /// any inline types.
  fn finalize_struct_types(
    &self,
    name: &str,
    schema: &ObjectSchema,
    main_type: RustType,
    inline_types: Vec<RustType>,
  ) -> anyhow::Result<Vec<RustType>> {
    let discriminated_enum = schema
      .is_discriminated_base_type()
      .then(|| {
        let base_struct_name = match &main_type {
          RustType::Struct(def) => def.name.as_str().to_string(),
          _ => format!("{}{DISCRIMINATED_BASE_SUFFIX}", to_rust_type_name(name)),
        };
        self
          .discriminator_converter
          .build_base_discriminated_enum(name, schema, &base_struct_name)
      })
      .transpose()?;

    Ok(
      discriminated_enum
        .into_iter()
        .chain(std::iter::once(main_type))
        .chain(inline_types)
        .collect(),
    )
  }
}
