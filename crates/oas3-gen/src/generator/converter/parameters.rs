use std::{collections::HashSet, rc::Rc};

use itertools::Itertools;
use oas3::spec::{Operation, Parameter, ParameterStyle};

use super::fields::FieldConverter;
use crate::generator::{
  ast::{
    FieldCollection as _, FieldDef, FieldNameToken, OuterAttr, ParameterLocation, ParsedPath, RustType, StructDef,
    StructKind, StructToken, TypeRef,
  },
  converter::ConverterContext,
  naming::constants::{
    HEADER_PARAMS_FIELD, HEADER_PARAMS_SUFFIX, PATH_PARAMS_FIELD, PATH_PARAMS_SUFFIX, QUERY_PARAMS_FIELD,
    QUERY_PARAMS_SUFFIX,
  },
};

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
  field_converter: FieldConverter,
}

impl ParameterConverter {
  /// Creates a new parameter converter.
  pub(crate) fn new(context: &Rc<ConverterContext>) -> Self {
    Self {
      context: context.clone(),
      field_converter: FieldConverter::new(context),
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
    let mut declared_path_params = HashSet::new();

    for param in self.collect_parameters(path, operation) {
      let location: ParameterLocation = param.location.into();

      if location == ParameterLocation::Path {
        declared_path_params.insert(param.name.clone());
      }

      let parent_name = collector.parent_name(location);

      let (field, types) = self.convert_parameter(&param, location, parent_name, &mut warnings)?;
      inline_types.extend(types);
      collector.insert(location, field);
    }

    for field in Self::synthesize_missing_fields(path, &declared_path_params) {
      collector.insert(ParameterLocation::Path, field);
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
    let spec = self.context.graph().spec();
    let mut params = vec![];

    if let Some(path_item) = spec.paths.as_ref().and_then(|p| p.get(path)) {
      params.extend(path_item.parameters.iter().filter_map(|r| r.resolve(spec).ok()));
    }

    for param in operation.parameters.iter().filter_map(|r| r.resolve(spec).ok()) {
      params.retain(|p| p.location != param.location || p.name != param.name);
      params.push(param);
    }

    params
  }

  fn synthesize_missing_fields(path: &str, declared: &HashSet<String>) -> impl Iterator<Item = FieldDef> {
    ParsedPath::extract_template_params(path)
      .filter(|name| !declared.contains(*name))
      .unique()
      .map(|name| FieldDef::builder().synthesized_path_param(name).build())
  }

  fn convert_parameter(
    &self,
    param: &Parameter,
    location: ParameterLocation,
    parent_name: &str,
    warnings: &mut Vec<String>,
  ) -> anyhow::Result<(FieldDef, Vec<RustType>)> {
    let Some(schema_ref) = param.schema.as_ref() else {
      warnings.push(format!(
        "Parameter '{}' has no schema, defaulting to String",
        param.name
      ));
      let field = FieldDef::builder()
        .name(FieldNameToken::from_raw(&param.name))
        .parameter(param, location)
        .rust_type(TypeRef::new("String").with_option())
        .build();
      return Ok((field, vec![]));
    };

    let is_required = param.required.unwrap_or(false);
    let resolved = self
      .field_converter
      .resolve_with_metadata(parent_name, &param.name, schema_ref, is_required)?;

    let rust_type = if is_required {
      resolved.type_ref
    } else {
      resolved.type_ref.with_option()
    };

    let mut field = FieldDef::builder()
      .name(FieldNameToken::from_raw(&param.name))
      .parameter_with_schema(param, location, &resolved.schema)
      .rust_type(rust_type)
      .validation_attrs(resolved.validation_attrs)
      .build();

    if location == ParameterLocation::Query {
      let explode = param
        .explode
        .unwrap_or(matches!(param.style, None | Some(ParameterStyle::Form)));
      field = field.with_serde_attributes(explode, param.style);
    }

    Ok((field, resolved.inline_types))
  }
}

#[derive(Debug, Clone)]
struct ParamGroup {
  struct_name: String,
  field_name: &'static str,
  kind: StructKind,
  fields: Vec<FieldDef>,
}

impl ParamGroup {
  fn new(request_name: &str, suffix: &str, field_name: &'static str, kind: StructKind) -> Self {
    Self {
      struct_name: format!("{request_name}{suffix}"),
      field_name,
      kind,
      fields: vec![],
    }
  }

  fn into_structs(self) -> Option<(FieldDef, StructDef)> {
    if self.fields.is_empty() {
      return None;
    }

    let outer_attrs = if self.kind == StructKind::QueryParams && self.fields.has_serde_as() {
      vec![OuterAttr::SerdeAs]
    } else {
      vec![]
    };

    let nested = StructDef::builder()
      .name(StructToken::new(&self.struct_name))
      .fields(self.fields)
      .outer_attrs(outer_attrs)
      .kind(self.kind)
      .build();

    let main = FieldDef::nested_struct_field(self.field_name, &self.struct_name);

    Some((main, nested))
  }
}

#[derive(Debug, Clone)]
struct Collector {
  path: ParamGroup,
  query: ParamGroup,
  header: ParamGroup,
  all: Vec<FieldDef>,
}

impl Collector {
  fn new(request_name: &str) -> Self {
    Self {
      path: ParamGroup::new(
        request_name,
        PATH_PARAMS_SUFFIX,
        PATH_PARAMS_FIELD,
        StructKind::PathParams,
      ),
      query: ParamGroup::new(
        request_name,
        QUERY_PARAMS_SUFFIX,
        QUERY_PARAMS_FIELD,
        StructKind::QueryParams,
      ),
      header: ParamGroup::new(
        request_name,
        HEADER_PARAMS_SUFFIX,
        HEADER_PARAMS_FIELD,
        StructKind::HeaderParams,
      ),
      all: vec![],
    }
  }

  fn parent_name(&self, location: ParameterLocation) -> &str {
    match location {
      ParameterLocation::Path => &self.path.struct_name,
      ParameterLocation::Query | ParameterLocation::Cookie => &self.query.struct_name,
      ParameterLocation::Header => &self.header.struct_name,
    }
  }

  fn insert(&mut self, location: ParameterLocation, field: FieldDef) {
    self.all.push(field.clone());
    match location {
      ParameterLocation::Path => self.path.fields.push(field),
      ParameterLocation::Query => self.query.fields.push(field),
      ParameterLocation::Header => self.header.fields.push(field),
      ParameterLocation::Cookie => {}
    }
  }

  fn finish(self) -> (Vec<FieldDef>, Vec<StructDef>, Vec<FieldDef>) {
    let (main_fields, nested_structs) = [self.path, self.query, self.header]
      .into_iter()
      .filter_map(ParamGroup::into_structs)
      .unzip();

    (main_fields, nested_structs, self.all)
  }
}
