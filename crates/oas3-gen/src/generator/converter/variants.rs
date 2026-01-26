use std::rc::Rc;

use oas3::spec::ObjectSchema;

use super::{ConversionOutput, type_resolver::TypeResolver, union_types::UnionVariantSpec};
use crate::{
  generator::{
    ast::{Documentation, EnumVariantToken, SerdeAttribute, TypeRef, VariantContent, VariantDef},
    converter::ConverterContext,
    naming::{identifiers::to_rust_type_name, inference::NormalizedVariant},
  },
  utils::SchemaExt,
};

#[derive(Clone, Debug)]
pub(crate) struct VariantBuilder {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
}

impl VariantBuilder {
  /// Creates a new `VariantBuilder` with access to the schema registry and type conversion facilities.
  ///
  /// The `context` provides access to the schema dependency graph, shared type cache,
  /// and configuration. The builder uses this to resolve schema references and detect
  /// cyclic types that require boxing.
  pub(crate) fn new(context: Rc<ConverterContext>) -> Self {
    let type_resolver = TypeResolver::new(context.clone());
    Self { context, type_resolver }
  }

  /// Converts a `UnionVariantSpec` into a Rust `VariantDef` for use in an enum.
  ///
  /// Dispatches to specialized builders based on whether the variant references a named
  /// schema (producing a tuple variant like `Foo(FooType)`) or contains an inline schema
  /// (which may produce unit, tuple, or struct-like variants depending on schema content).
  ///
  /// Returns a `ConversionOutput` containing the variant definition and any inline types
  /// that were generated during conversion (such as nested structs or enums).
  pub(crate) fn build_variant(
    &self,
    enum_name: &str,
    spec: &UnionVariantSpec,
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    if let Some(ref schema_name) = spec.ref_name {
      Ok(self.build_ref_variant(schema_name, spec))
    } else {
      self.build_inline_variant(&spec.resolved_schema, enum_name, &spec.variant_name)
    }
  }

  /// Builds a tuple variant that wraps a reference to a named schema type.
  ///
  /// For a schema reference like `$ref: "#/components/schemas/Pet"`, this produces
  /// a variant like `Pet(Pet)` or `Pet(Box<Pet>)` if the type participates in a
  /// dependency cycle.
  ///
  /// The type reference is wrapped with `unwrap_option()` to allow null values, and
  /// `with_boxed()` is applied for cyclic types to break infinite recursion during
  /// struct layout computation.
  fn build_ref_variant(&self, schema_name: &str, spec: &UnionVariantSpec) -> ConversionOutput<VariantDef> {
    let rust_type_name = to_rust_type_name(schema_name);

    let type_ref = if self.context.graph().is_cyclic(schema_name) {
      TypeRef::new(&rust_type_name).unwrap_option().with_boxed()
    } else {
      TypeRef::new(&rust_type_name).unwrap_option()
    };

    ConversionOutput::new(
      VariantDef::builder()
        .name(spec.variant_name.clone())
        .docs(Documentation::from_optional(spec.resolved_schema.description.as_ref()))
        .content(VariantContent::Tuple(vec![type_ref]))
        .deprecated(spec.resolved_schema.deprecated.unwrap_or(false))
        .build(),
    )
  }

  /// Builds an enum variant from an inline schema definition.
  ///
  /// Analyzes the schema structure to determine the appropriate variant representation:
  /// - Schemas with `const` values become unit variants with serde rename attributes
  /// - Schemas with `properties` generate inline struct types wrapped in a tuple variant
  /// - Array schemas produce tuple variants wrapping `Vec<T>`
  /// - Union schemas (nested oneOf/anyOf) produce tuple variants wrapping nested enums
  /// - Primitive schemas produce tuple variants wrapping the primitive type
  ///
  /// Any inline struct or enum types generated during conversion are included in
  /// the returned `ConversionOutput::inline_types`.
  fn build_inline_variant(
    &self,
    resolved_schema: &ObjectSchema,
    enum_name: &str,
    variant_name: &EnumVariantToken,
  ) -> anyhow::Result<ConversionOutput<VariantDef>> {
    let resolved_schema = if resolved_schema.has_intersection() {
      self.context.graph().merge_inline(resolved_schema)?
    } else {
      resolved_schema.clone()
    };

    if let Some(output) = Self::build_const_content(&resolved_schema, variant_name)? {
      return Ok(output);
    }

    let variant_label = variant_name.to_string();

    let content_output = if resolved_schema.properties.is_empty() {
      if let Some(output) = self.build_array_content(enum_name, &variant_label, &resolved_schema)? {
        output
      } else if let Some(output) = self.build_nested_union_content(enum_name, &variant_label, &resolved_schema)? {
        output
      } else {
        self.build_primitive_content(&resolved_schema)?
      }
    } else {
      self.build_struct_content(enum_name, &variant_label, &resolved_schema)?
    };

    Ok(ConversionOutput::with_inline_types(
      VariantDef::builder()
        .name(variant_name.clone())
        .content(content_output.result)
        .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
        .deprecated(resolved_schema.deprecated.unwrap_or(false))
        .build(),
      content_output.inline_types,
    ))
  }

  /// Builds a unit variant for schemas that define a constant value.
  ///
  /// For schemas like `{ "const": "active" }`, this produces a unit variant with a
  /// `#[serde(rename = "active")]` attribute. The variant name is derived from the
  /// `variant_name` parameter, while the actual serialized value comes from the const.
  ///
  /// Returns `None` if the schema has no `const_value`, allowing callers to fall through
  /// to other variant-building strategies. Returns an error if the const value type
  /// is not supported (e.g., non-string constants).
  fn build_const_content(
    resolved_schema: &ObjectSchema,
    variant_name: &EnumVariantToken,
  ) -> anyhow::Result<Option<ConversionOutput<VariantDef>>> {
    let Some(const_value) = &resolved_schema.const_value else {
      return Ok(None);
    };

    let normalized = NormalizedVariant::try_from(const_value)
      .map_err(|_| anyhow::anyhow!("Unsupported const value type: {const_value}"))?;

    let variant = VariantDef::builder()
      .name(variant_name.clone())
      .docs(Documentation::from_optional(resolved_schema.description.as_ref()))
      .content(VariantContent::Unit)
      .serde_attrs(vec![SerdeAttribute::Rename(normalized.rename_value)])
      .deprecated(resolved_schema.deprecated.unwrap_or(false))
      .build();

    Ok(Some(ConversionOutput::new(variant)))
  }

  /// Builds tuple variant content for array-typed schemas.
  ///
  /// For a schema like `{ "type": "array", "items": { "$ref": "..." } }`, this produces
  /// `VariantContent::Tuple(vec![Vec<ItemType>])`. If the array items are inline schemas,
  /// the generated item types are included in the returned `ConversionOutput::inline_types`.
  ///
  /// Returns `None` if the schema is not an array, allowing callers to try other
  /// content-building strategies.
  fn build_array_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.is_array() {
      return Ok(None);
    }

    let conversion = self
      .type_resolver
      .try_inline_array(enum_name, variant_label, resolved_schema)?;

    Ok(conversion.map(|c| ConversionOutput::with_inline_types(VariantContent::Tuple(vec![c.result]), c.inline_types)))
  }

  /// Builds tuple variant content for schemas containing nested oneOf/anyOf unions.
  ///
  /// When a union variant itself contains a union (e.g., a oneOf within a oneOf),
  /// this generates a separate enum type for the nested union and wraps it in a tuple
  /// variant. The generated enum is included in `ConversionOutput::inline_types`.
  ///
  /// Returns `None` if the schema has no union keywords, allowing callers to try
  /// other content-building strategies.
  fn build_nested_union_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<Option<ConversionOutput<VariantContent>>> {
    if !resolved_schema.has_union() {
      return Ok(None);
    }

    let result = self
      .type_resolver
      .inline_union(enum_name, variant_label, resolved_schema)?;

    Ok(Some(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    )))
  }

  /// Builds tuple variant content for object schemas with properties.
  ///
  /// For an inline schema with fields, this generates a dedicated struct type named
  /// `{EnumName}{VariantLabel}` containing the schema's properties. The variant then
  /// wraps this struct in a tuple, e.g., `VariantA(MyEnumVariantA)`.
  ///
  /// The generated struct definition is included in `ConversionOutput::inline_types`
  /// for emission alongside the parent enum.
  fn build_struct_content(
    &self,
    enum_name: &str,
    variant_label: &str,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let enum_name_converted = to_rust_type_name(enum_name);
    let struct_name_prefix = format!("{enum_name_converted}{variant_label}");

    let result = self
      .type_resolver
      .inline_struct_from_schema(resolved_schema, &struct_name_prefix)?;

    Ok(ConversionOutput::with_inline_types(
      VariantContent::Tuple(vec![result.result]),
      result.inline_types,
    ))
  }

  /// Builds tuple variant content for primitive-typed schemas.
  ///
  /// For schemas with primitive types like `{ "type": "string" }` or `{ "type": "integer" }`,
  /// this produces `VariantContent::Tuple(vec![String])` or similar. The type reference
  /// is unwrapped from Option to handle nullable values directly.
  ///
  /// This is the fallback for inline schemas that are not arrays, unions, or objects
  /// with properties.
  fn build_primitive_content(
    &self,
    resolved_schema: &ObjectSchema,
  ) -> anyhow::Result<ConversionOutput<VariantContent>> {
    let type_ref = self.type_resolver.resolve_type(resolved_schema)?.unwrap_option();
    Ok(ConversionOutput::new(VariantContent::Tuple(vec![type_ref])))
  }
}
