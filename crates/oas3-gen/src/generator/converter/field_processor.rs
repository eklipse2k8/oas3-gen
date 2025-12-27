use oas3::spec::{ObjectSchema, Schema};

use super::{
  CodegenConfig,
  discriminator::{DiscriminatorInfo, apply_discriminator_attributes, get_discriminator_info},
  metadata::FieldMetadata,
  type_resolver::TypeResolver,
};
use crate::generator::{
  ast::{Documentation, FieldDef, FieldDefBuilder, SerdeAttribute, TypeRef, tokens::FieldNameToken},
  naming::identifiers::to_rust_field_name,
};

pub(crate) struct AdditionalPropertiesResult {
  pub serde_attrs: Vec<SerdeAttribute>,
  pub additional_field: Option<FieldDef>,
}

#[derive(Clone)]
pub(crate) struct FieldProcessor {
  odata_support: bool,
  type_resolver: TypeResolver,
}

impl FieldProcessor {
  pub(crate) fn new(config: CodegenConfig, type_resolver: TypeResolver) -> Self {
    Self {
      odata_support: config.odata_support(),
      type_resolver,
    }
  }

  pub(crate) fn process_single_field(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    resolved_type: TypeRef,
    is_required: bool,
    discriminator_mapping: Option<&(String, String)>,
  ) -> anyhow::Result<FieldDef> {
    let discriminator_info = get_discriminator_info(prop_name, parent_schema, prop_schema, discriminator_mapping);

    let should_be_optional = self.is_field_optional(
      prop_name,
      parent_schema,
      prop_schema,
      discriminator_info.as_ref(),
      is_required,
    );
    let final_type = if should_be_optional && !resolved_type.nullable {
      resolved_type.with_option()
    } else {
      resolved_type
    };

    let metadata = FieldMetadata::from_schema(prop_name, is_required, prop_schema, &final_type);
    let rust_field_name = to_rust_field_name(prop_name);
    let serde_attrs = if rust_field_name == prop_name {
      vec![]
    } else {
      vec![SerdeAttribute::Rename(prop_name.to_string())]
    };

    let disc_attrs = apply_discriminator_attributes(metadata, serde_attrs, &final_type, discriminator_info.as_ref());

    let field = FieldDefBuilder::default()
      .name(to_rust_field_name(prop_name))
      .rust_type(final_type)
      .docs(disc_attrs.metadata.docs)
      .serde_attrs(disc_attrs.serde_attrs)
      .doc_hidden(disc_attrs.doc_hidden)
      .validation_attrs(disc_attrs.metadata.validation_attrs)
      .default_value(disc_attrs.metadata.default_value)
      .deprecated(disc_attrs.metadata.deprecated)
      .multiple_of(disc_attrs.metadata.multiple_of)
      .build()?;

    Ok(field)
  }

  fn is_field_optional(
    &self,
    prop_name: &str,
    parent_schema: &ObjectSchema,
    prop_schema: &ObjectSchema,
    discriminator_info: Option<&DiscriminatorInfo>,
    is_required: bool,
  ) -> bool {
    let has_default = prop_schema.default.is_some();
    let is_discriminator_field = discriminator_info.is_some();
    let discriminator_has_enum = discriminator_info.is_some_and(|i| i.has_enum);

    if !is_required || has_default {
      return true;
    }
    if is_discriminator_field && !discriminator_has_enum {
      return true;
    }
    if self.odata_support
      && prop_name.starts_with("@odata.")
      && parent_schema.discriminator.is_none()
      && parent_schema.all_of.is_empty()
    {
      return true;
    }
    false
  }

  pub(crate) fn prepare_additional_properties(
    &self,
    schema: &ObjectSchema,
  ) -> anyhow::Result<AdditionalPropertiesResult> {
    let mut serde_attrs = vec![];
    let mut additional_field = None;

    if let Some(ref additional) = schema.additional_properties {
      match additional {
        Schema::Boolean(b) if !b.0 => {
          serde_attrs.push(SerdeAttribute::DenyUnknownFields);
        }
        Schema::Object(_) => {
          let value_type = self.type_resolver.resolve_additional_properties_type(additional)?;
          let map_type = TypeRef::new(format!(
            "std::collections::HashMap<String, {}>",
            value_type.to_rust_type()
          ));
          additional_field = Some(FieldDef {
            name: FieldNameToken::new("additional_properties"),
            docs: Documentation::from_lines(["Additional properties not defined in the schema."]),
            rust_type: map_type,
            serde_attrs: vec![SerdeAttribute::Flatten],
            ..Default::default()
          });
        }
        Schema::Boolean(_) => {}
      }
    }
    Ok(AdditionalPropertiesResult {
      serde_attrs,
      additional_field,
    })
  }
}
