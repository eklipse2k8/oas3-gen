use std::collections::BTreeSet;

use itertools::Itertools;
use oas3::spec::{ObjectSchema, Parameter, ParameterStyle};

use crate::generator::ast::{
  Documentation, FieldNameToken, OuterAttr, ParameterLocation, RustPrimitive, SerdeAsFieldAttr, SerdeAsSeparator,
  SerdeAttribute, TypeRef, ValidationAttribute, bon_attrs::BuilderAttribute,
};

/// Rust struct field definition
#[derive(Debug, Clone, Default, PartialEq, Eq, bon::Builder)]
pub struct FieldDef {
  pub name: FieldNameToken,
  #[builder(default)]
  pub docs: Documentation,
  pub rust_type: TypeRef,
  #[builder(default)]
  pub serde_attrs: BTreeSet<SerdeAttribute>,
  pub serde_as_attr: Option<SerdeAsFieldAttr>,
  #[builder(default)]
  pub doc_hidden: bool,
  #[builder(default)]
  pub validation_attrs: Vec<ValidationAttribute>,
  #[builder(default)]
  pub builder_attrs: Vec<BuilderAttribute>,
  pub default_value: Option<serde_json::Value>,
  pub example_value: Option<serde_json::Value>,
  #[builder(into)]
  pub parameter_location: Option<ParameterLocation>,
  #[builder(default)]
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
  #[builder(into)]
  pub original_name: Option<String>,
}

impl FieldDef {
  #[must_use]
  pub fn is_required(&self) -> bool {
    self.default_value.is_none() && !self.rust_type.nullable
  }

  #[must_use]
  pub fn with_discriminator_behavior(mut self, discriminator_value: Option<&str>, is_base: bool) -> Self {
    self.docs.clear();
    self.validation_attrs.clear();
    self.doc_hidden = true;

    if self.rust_type.base_type == RustPrimitive::String {
      self.rust_type.base_type = RustPrimitive::StaticStr;
    }

    if let Some(value) = discriminator_value {
      self.default_value = Some(serde_json::Value::String(value.to_string()));
      self.serde_attrs.insert(SerdeAttribute::SkipDeserializing);
      self.serde_attrs.insert(SerdeAttribute::Default);
    } else if is_base {
      self.serde_attrs.clear();
      self.serde_attrs.insert(SerdeAttribute::Skip);
      if self.rust_type.is_string_like() {
        self.default_value = Some(serde_json::Value::String(String::new()));
      }
    }

    self
  }

  #[must_use]
  pub fn with_serde_attributes(mut self, explode: bool, style: Option<ParameterStyle>) -> Self {
    if let Some(original) = &self.original_name
      && self.name != original.as_str()
    {
      self.serde_attrs.insert(SerdeAttribute::Rename(original.clone()));
    }

    if self.rust_type.is_array && !explode {
      let separator = match style {
        Some(ParameterStyle::SpaceDelimited) => SerdeAsSeparator::Space,
        Some(ParameterStyle::PipeDelimited) => SerdeAsSeparator::Pipe,
        _ => SerdeAsSeparator::Comma,
      };

      self.serde_as_attr = Some(SerdeAsFieldAttr::SeparatedList {
        separator,
        optional: self.rust_type.nullable,
      });
    }

    self
  }

  #[must_use]
  pub fn with_builder_attrs(self) -> Self {
    let mut attrs = vec![];

    let needs_rename = BON_RESERVED_FIELD_NAMES.contains(&self.name.as_str());
    if needs_rename && !self.doc_hidden {
      attrs.push(BuilderAttribute::Rename(format!("{}_value", self.name.as_str())));
    }

    if let Some(default_value) = &self.default_value {
      if self.doc_hidden {
        attrs.push(BuilderAttribute::Skip {
          value: default_value.clone(),
          type_ref: self.rust_type.clone(),
        });
      } else if !self.rust_type.nullable {
        attrs.push(BuilderAttribute::Default {
          value: default_value.clone(),
          type_ref: self.rust_type.clone(),
        });
      }
    }

    let mut new_field = self;
    new_field.builder_attrs = attrs;
    new_field
  }
}

pub trait FieldCollection: Send + Sync {
  #[must_use]
  fn has_serde_as(&self) -> bool;

  /// Derives struct-level serde attributes from field characteristics.
  ///
  /// Appends `#[serde(default)]` to `base` when any field carries a default value.
  #[must_use]
  fn struct_serde_attrs(&self, base: Vec<SerdeAttribute>) -> Vec<SerdeAttribute>;

  /// Derives struct-level outer attributes from field characteristics.
  ///
  /// Returns `#[serde_as]` when any field uses serde_with custom serialization.
  #[must_use]
  fn struct_outer_attrs(&self) -> Vec<OuterAttr>;
}

use crate::generator::{
  ast::fields::field_def_builder::{
    IsSet, IsUnset, SetDefaultValue, SetDeprecated, SetDocs, SetExampleValue, SetMultipleOf, SetName, SetOriginalName,
    SetParameterLocation, SetRustType, SetSerdeAttrs, State,
  },
  naming::constants::BON_RESERVED_FIELD_NAMES,
};

impl<S: State> FieldDefBuilder<S>
where
  S::Deprecated: IsUnset,
  S::Docs: IsUnset,
  S::ExampleValue: IsUnset,
  S::MultipleOf: IsUnset,
{
  pub fn schema(
    self,
    schema: &ObjectSchema,
  ) -> FieldDefBuilder<SetMultipleOf<SetExampleValue<SetDocs<SetDeprecated<S>>>>> {
    self
      .deprecated(schema.deprecated.unwrap_or(false))
      .docs(Documentation::from_optional(schema.description.as_ref()))
      .maybe_example_value(schema.example.clone())
      .maybe_multiple_of(schema.multiple_of.clone())
  }
}

impl<S: State> FieldDefBuilder<S>
where
  S::Name: IsSet,
  S::Docs: IsUnset,
  S::ParameterLocation: IsUnset,
  S::OriginalName: IsUnset,
{
  pub fn parameter(
    self,
    param: &Parameter,
    location: ParameterLocation,
  ) -> FieldDefBuilder<SetOriginalName<SetParameterLocation<SetDocs<S>>>> {
    self
      .docs(Documentation::from_optional(param.description.as_ref()))
      .parameter_location(location)
      .original_name(param.name.clone())
  }
}

impl<S: State> FieldDefBuilder<S>
where
  S::Name: IsUnset,
  S::RustType: IsUnset,
  S::ParameterLocation: IsUnset,
  S::OriginalName: IsUnset,
{
  /// Creates a field for a synthesized path parameter extracted from a URL template.
  pub fn synthesized_path_param(
    self,
    name: &str,
  ) -> FieldDefBuilder<SetOriginalName<SetParameterLocation<SetRustType<SetName<S>>>>> {
    self
      .name(FieldNameToken::from_raw(name))
      .rust_type(TypeRef::new("String"))
      .parameter_location(ParameterLocation::Path)
      .original_name(name.to_string())
  }
}

impl<S: State> FieldDefBuilder<S>
where
  S::Name: IsUnset,
  S::Docs: IsUnset,
  S::RustType: IsUnset,
  S::SerdeAttrs: IsUnset,
{
  pub fn additional_properties(
    self,
    value_type: &TypeRef,
  ) -> FieldDefBuilder<SetSerdeAttrs<SetRustType<SetDocs<SetName<S>>>>> {
    self
      .name(FieldNameToken::from_raw("additional_properties"))
      .docs(Documentation::from_lines([
        "Additional properties not defined in the schema.",
      ]))
      .rust_type(TypeRef::new(format!(
        "std::collections::HashMap<String, {}>",
        value_type.to_rust_type()
      )))
      .serde_attrs(BTreeSet::from([SerdeAttribute::Flatten]))
  }
}

impl<S: State> FieldDefBuilder<S>
where
  S::Name: IsSet,
  S::Deprecated: IsUnset,
  S::Docs: IsUnset,
  S::ExampleValue: IsUnset,
  S::DefaultValue: IsUnset,
  S::MultipleOf: IsUnset,
  S::ParameterLocation: IsUnset,
  S::OriginalName: IsUnset,
{
  /// Sets parameter metadata from both the Parameter and its resolved ObjectSchema.
  ///
  /// Precedence: param.description > schema.description, param.example > schema.example.
  /// Default value uses schema.default > schema.const_value > single enum value.
  #[allow(clippy::type_complexity)]
  pub fn parameter_with_schema(
    self,
    param: &Parameter,
    location: ParameterLocation,
    schema: &ObjectSchema,
  ) -> FieldDefBuilder<
    SetOriginalName<SetParameterLocation<SetMultipleOf<SetDefaultValue<SetExampleValue<SetDocs<SetDeprecated<S>>>>>>>,
  > {
    let docs = Documentation::from_optional(param.description.as_ref().or(schema.description.as_ref()));

    let example_value = param.example.clone().or_else(|| schema.example.clone());

    let default_value = schema
      .default
      .clone()
      .or_else(|| schema.const_value.clone())
      .or_else(|| schema.enum_values.iter().exactly_one().ok().cloned());

    self
      .deprecated(schema.deprecated.unwrap_or(false))
      .docs(docs)
      .maybe_example_value(example_value)
      .maybe_default_value(default_value)
      .maybe_multiple_of(schema.multiple_of.clone())
      .parameter_location(location)
      .original_name(param.name.clone())
  }
}

impl FieldDef {
  /// Renames the field to the specified new name.
  #[must_use]
  pub fn renamed_to(self, new_name: &str) -> Self {
    let mut new_field = self;
    new_field.name = FieldNameToken::from_raw(new_name);
    new_field
  }

  /// Creates a field for a request body.
  #[must_use]
  pub fn body_field(field_name: &str, description: Option<&String>, type_ref: TypeRef, optional: bool) -> Self {
    let rust_type = if optional { type_ref.with_option() } else { type_ref };
    Self {
      name: FieldNameToken::from_raw(field_name),
      docs: Documentation::from_optional(description),
      rust_type,
      ..Default::default()
    }
  }

  /// Creates a field that references a nested struct (e.g., for parameter groups).
  #[must_use]
  pub fn nested_struct_field(field_name: &str, struct_name: &str) -> Self {
    Self {
      name: FieldNameToken::from_raw(field_name),
      rust_type: TypeRef::new(struct_name),
      ..Default::default()
    }
  }
}

impl FieldCollection for [FieldDef] {
  fn has_serde_as(&self) -> bool {
    self.iter().any(|t| t.serde_as_attr.is_some())
  }

  fn struct_serde_attrs(&self, base: Vec<SerdeAttribute>) -> Vec<SerdeAttribute> {
    let default_serde = self
      .iter()
      .any(|f| f.default_value.is_some())
      .then_some(SerdeAttribute::Default);

    base.into_iter().chain(default_serde).collect()
  }

  fn struct_outer_attrs(&self) -> Vec<OuterAttr> {
    self.has_serde_as().then_some(OuterAttr::SerdeAs).into_iter().collect()
  }
}
