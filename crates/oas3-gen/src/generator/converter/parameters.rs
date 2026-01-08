use std::{
  collections::{BTreeMap, HashSet},
  rc::Rc,
};

use oas3::spec::{
  ObjectOrReference, ObjectSchema, Operation, Parameter, ParameterIn, ParameterStyle, SchemaType, SchemaTypeSet,
};
use serde_json::Value;

use super::{SchemaExt, TypeResolver, fields::FieldConverter};
use crate::generator::{
  ast::{
    Documentation, FieldCollection as _, FieldDef, FieldNameToken, OuterAttr, ParameterLocation, ParsedPath, RustType,
    StructDef, StructKind, StructToken, TypeRef, ValidationAttribute,
  },
  converter::ConverterContext,
  naming::constants::{
    HEADER_PARAMS_FIELD, HEADER_PARAMS_SUFFIX, PATH_PARAMS_FIELD, PATH_PARAMS_SUFFIX, QUERY_PARAMS_FIELD,
    QUERY_PARAMS_SUFFIX,
  },
};

/// Resolved parameter type with validation and inline types.
type ResolvedParam = (TypeRef, Vec<ValidationAttribute>, Option<Value>, Vec<RustType>);

#[derive(Debug, Clone)]
pub(crate) struct ConvertedParams {
  pub(crate) main_fields: Vec<FieldDef>,
  pub(crate) nested_structs: Vec<StructDef>,
  pub(crate) all_fields: Vec<FieldDef>,
  pub(crate) inline_types: Vec<RustType>,
  pub(crate) warnings: Vec<String>,
}

/// Converts OpenAPI parameters into Rust field definitions.
///
/// Groups parameters by location (path, query, header) and generates
/// nested structs for each group.
#[derive(Debug, Clone)]
pub(crate) struct ParameterConverter {
  context: Rc<ConverterContext>,
  type_resolver: TypeResolver,
}

impl ParameterConverter {
  /// Creates a new parameter converter.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      context: context.clone(),
      type_resolver: TypeResolver::new(context.clone()),
    }
  }

  /// Converts all parameters for an operation.
  ///
  /// Collects parameters from both path-level and operation-level definitions,
  /// synthesizes missing path parameters, and groups them into nested structs.
  pub(crate) fn convert_all(
    &self,
    request_name: &str,
    path: &str,
    operation: &Operation,
  ) -> anyhow::Result<ConvertedParams> {
    let mut collector = Collector::new(request_name);
    let mut inline_types = vec![];
    let mut warnings = vec![];

    for param in self.collect_parameters(path, operation) {
      let location: ParameterLocation = param.location.into();
      let parent_name = collector.parent_name(location);

      let (field, types) = self.convert_parameter(&param, location, parent_name, &mut warnings)?;
      inline_types.extend(types);
      collector.insert(location, field);
    }

    let (main_fields, nested_structs, all_fields) = collector.finish();

    Ok(ConvertedParams {
      main_fields,
      nested_structs,
      all_fields,
      inline_types,
      warnings,
    })
  }

  fn collect_parameters(&self, path: &str, operation: &Operation) -> Vec<Parameter> {
    let mut params = vec![];

    if let Some(path_item) = self.context.graph().spec().paths.as_ref().and_then(|p| p.get(path)) {
      for param_ref in &path_item.parameters {
        if let Ok(param) = param_ref.resolve(self.context.graph().spec()) {
          params.push(param);
        }
      }
    }

    for param_ref in &operation.parameters {
      if let Ok(param) = param_ref.resolve(self.context.graph().spec()) {
        let key = (param.location, param.name.clone());
        params.retain(|p| (p.location, p.name.clone()) != key);
        params.push(param);
      }
    }

    params.extend(Self::synthesize_missing(path, &params));
    params
  }

  fn synthesize_missing(path: &str, existing: &[Parameter]) -> Vec<Parameter> {
    let declared: HashSet<_> = existing
      .iter()
      .filter(|p| p.location == ParameterIn::Path)
      .map(|p| p.name.as_str())
      .collect();

    ParsedPath::extract_template_params(path)
      .filter(|name| !declared.contains(name))
      .map(|name| Parameter {
        name: name.to_string(),
        location: ParameterIn::Path,
        description: None,
        required: Some(true),
        deprecated: None,
        allow_empty_value: None,
        style: None,
        explode: None,
        allow_reserved: None,
        schema: Some(ObjectOrReference::Object(ObjectSchema {
          schema_type: Some(SchemaTypeSet::Single(SchemaType::String)),
          ..Default::default()
        })),
        example: None,
        examples: BTreeMap::new(),
        content: None,
        extensions: BTreeMap::new(),
      })
      .collect()
  }

  fn convert_parameter(
    &self,
    param: &Parameter,
    location: ParameterLocation,
    parent_name: &str,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<(FieldDef, Vec<RustType>)> {
    let (type_ref, validation_attrs, default_value, inline_types) = self.resolve_type(param, parent_name, warnings)?;

    let is_required = param.required.unwrap_or(false);
    let rust_type = if is_required { type_ref } else { type_ref.with_option() };

    let mut field = FieldDef::builder()
      .name(FieldNameToken::from_raw(&param.name))
      .docs(Documentation::from_optional(param.description.as_ref()))
      .rust_type(rust_type)
      .validation_attrs(validation_attrs)
      .maybe_default_value(default_value)
      .maybe_example_value(param.example.clone())
      .parameter_location(location)
      .original_name(param.name.clone())
      .build();

    if location == ParameterLocation::Query {
      let explode = param
        .explode
        .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)));
      field = field.with_serde_attributes(explode, param.style);
    }

    Ok((field, inline_types))
  }

  fn resolve_type(
    &self,
    param: &Parameter,
    parent_name: &str,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<ResolvedParam> {
    let Some(schema_ref) = param.schema.as_ref() else {
      warnings.push(format!(
        "Parameter '{}' has no schema, defaulting to String",
        param.name
      ));
      return Ok((TypeRef::new("String"), vec![], None, vec![]));
    };

    let schema = schema_ref.resolve(self.context.graph().spec())?;
    let has_inline_enum = schema.enum_values.len() > 1
      || schema
        .inline_array_items(self.context.graph().spec())
        .is_some_and(|items| items.enum_values.len() > 1);

    let (type_ref, inline_types) = if has_inline_enum {
      let result = self
        .type_resolver
        .resolve_property(parent_name, &param.name, &schema, schema_ref)?;
      (result.result, result.inline_types)
    } else {
      (self.type_resolver.resolve_type(&schema)?, vec![])
    };

    let is_required = param.required.unwrap_or(false);
    let (validation_attrs, default_value) =
      FieldConverter::extract_parameter_metadata(&param.name, is_required, &schema, &type_ref);

    Ok((type_ref, validation_attrs, default_value, inline_types))
  }
}

#[derive(Debug, Clone)]
struct Collector {
  path_struct_name: String,
  query_struct_name: String,
  header_struct_name: String,
  path: Vec<FieldDef>,
  query: Vec<FieldDef>,
  header: Vec<FieldDef>,
  all: Vec<FieldDef>,
}

impl Collector {
  fn new(request_name: &str) -> Self {
    Self {
      path_struct_name: format!("{request_name}{PATH_PARAMS_SUFFIX}"),
      query_struct_name: format!("{request_name}{QUERY_PARAMS_SUFFIX}"),
      header_struct_name: format!("{request_name}{HEADER_PARAMS_SUFFIX}"),
      path: vec![],
      query: vec![],
      header: vec![],
      all: vec![],
    }
  }

  fn parent_name(&self, location: ParameterLocation) -> &str {
    match location {
      ParameterLocation::Path => &self.path_struct_name,
      ParameterLocation::Query | ParameterLocation::Cookie => &self.query_struct_name,
      ParameterLocation::Header => &self.header_struct_name,
    }
  }

  fn insert(&mut self, location: ParameterLocation, field: FieldDef) {
    self.all.push(field.clone());
    match location {
      ParameterLocation::Path => self.path.push(field),
      ParameterLocation::Query => self.query.push(field),
      ParameterLocation::Header => self.header.push(field),
      ParameterLocation::Cookie => {}
    }
  }

  fn finish(self) -> (Vec<FieldDef>, Vec<StructDef>, Vec<FieldDef>) {
    let mut main_fields = vec![];
    let mut nested_structs = vec![];

    let groups = [
      (
        self.path,
        &self.path_struct_name,
        PATH_PARAMS_FIELD,
        StructKind::PathParams,
      ),
      (
        self.query,
        &self.query_struct_name,
        QUERY_PARAMS_FIELD,
        StructKind::QueryParams,
      ),
      (
        self.header,
        &self.header_struct_name,
        HEADER_PARAMS_FIELD,
        StructKind::HeaderParams,
      ),
    ];

    for (fields, struct_name, field_name, kind) in groups {
      if fields.is_empty() {
        continue;
      }

      let outer_attrs = if matches!(kind, StructKind::QueryParams) && fields.has_serde_as() {
        vec![OuterAttr::SerdeAs]
      } else {
        vec![]
      };

      nested_structs.push(
        StructDef::builder()
          .name(StructToken::new(struct_name))
          .fields(fields)
          .outer_attrs(outer_attrs)
          .kind(kind)
          .build(),
      );

      main_fields.push(
        FieldDef::builder()
          .name(FieldNameToken::from_raw(field_name))
          .rust_type(TypeRef::new(struct_name.clone()))
          .build(),
      );
    }

    (main_fields, nested_structs, self.all)
  }
}
