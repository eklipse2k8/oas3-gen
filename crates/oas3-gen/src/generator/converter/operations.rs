use std::rc::Rc;

use super::{
  ConverterContext, SchemaConverter,
  requests::{BodyInfo, RequestConverter, RequestOutput},
  responses::ResponseConverter,
};
use crate::generator::{
  ast::{
    Documentation, EnumToken, FieldDef, OperationInfo, ParameterLocation, ParsedPath, ResponseEnumDef, RustType,
    StructMethod, StructToken,
  },
  naming::{
    identifiers::to_rust_type_name,
    operations::{generate_unique_request_name, generate_unique_response_name},
  },
  operation_registry::OperationEntry,
};

#[derive(Debug, Clone)]
pub(crate) struct ConversionResult {
  pub(crate) types: Vec<RustType>,
  pub(crate) operation_info: OperationInfo,
}

type ResponseDefinition = (Option<ResponseEnumDef>, Option<StructMethod>);
type RequestTypes = (Vec<RustType>, Option<StructToken>);
type ResponseTypes = (Vec<RustType>, Option<EnumToken>);

/// Converts OpenAPI operations into Rust types and metadata.
///
/// Coordinates [`RequestConverter`] and [`ResponseConverter`] to transform
/// operation definitions into request/response type definitions.
#[derive(Debug, Clone)]
pub(crate) struct OperationConverter {
  context: Rc<ConverterContext>,
  schema_converter: SchemaConverter,
  response_converter: ResponseConverter,
  request_converter: RequestConverter,
}

impl OperationConverter {
  pub(crate) fn new(context: Rc<ConverterContext>, schema_converter: SchemaConverter) -> Self {
    let response_converter = ResponseConverter::new(context.clone());
    let request_converter = RequestConverter::new(&context);

    Self {
      context,
      schema_converter,
      response_converter,
      request_converter,
    }
  }

  pub(crate) fn convert(&self, entry: &OperationEntry) -> anyhow::Result<ConversionResult> {
    let base_name = to_rust_type_name(&entry.stable_id);
    let body_info = BodyInfo::new(&self.context, entry)?;

    self.context.mark_request_iter(&body_info.type_usage);

    let (response_def, parse_method) = self.response_definition(&base_name, entry);
    let request_output = self.request(&base_name, entry, &body_info, parse_method)?;

    let warnings = request_output.warnings.clone();
    let parameters = request_output.parameter_fields.clone();

    self.record_stats(&parameters);

    let (request_types, request_type) = self.request_types(request_output, response_def.is_some());
    let (response_types, response_enum_token) = self.response_types(response_def, request_type.as_ref());

    let types = Self::collect_types(&body_info, request_types, response_types);

    let operation_info = self.operation_info(
      entry,
      &base_name,
      request_type,
      response_enum_token,
      &body_info,
      warnings,
      parameters,
    )?;

    Ok(ConversionResult { types, operation_info })
  }

  fn response_definition(&self, base_name: &str, entry: &OperationEntry) -> ResponseDefinition {
    let response_name = generate_unique_response_name(base_name, |n| self.schema_converter.contains(n));
    let response_def = self
      .response_converter
      .build_enum(&response_name, &entry.operation, &entry.path);

    let parse_method = response_def.as_ref().map(|def| {
      self
        .response_converter
        .build_parse_method(&EnumToken::new(def.name.to_string()), &def.variants)
    });

    (response_def, parse_method)
  }

  fn request(
    &self,
    base_name: &str,
    entry: &OperationEntry,
    body_info: &BodyInfo,
    parse_method: Option<StructMethod>,
  ) -> anyhow::Result<RequestOutput> {
    let request_name = generate_unique_request_name(base_name, |n| self.schema_converter.contains(n));
    self
      .request_converter
      .build(&request_name, entry, body_info, parse_method)
  }

  fn request_types(&self, output: RequestOutput, has_response: bool) -> RequestTypes {
    let has_fields = !output.main_struct.fields.is_empty();

    if !has_fields && !has_response {
      return (vec![], None);
    }

    let name = output.main_struct.name.clone();

    let types = output
      .inline_types
      .into_iter()
      .chain(output.nested_structs.into_iter().map(RustType::Struct))
      .chain(std::iter::once(RustType::Struct(output.main_struct)))
      .collect::<Vec<_>>();

    self.context.mark_request(name.clone());

    (types, Some(name))
  }

  fn response_types(&self, response_def: Option<ResponseEnumDef>, request_type: Option<&StructToken>) -> ResponseTypes {
    let Some(mut def) = response_def else {
      return (vec![], None);
    };

    def.request_type = request_type.cloned();

    self.context.mark_response(def.name.clone());
    for schema_type in def.variants.iter().filter_map(|v| v.schema_type.as_ref()) {
      self.context.mark_response_type_ref(schema_type);
    }

    let token = EnumToken::new(def.name.to_string());
    (vec![RustType::ResponseEnum(def)], Some(token))
  }

  fn record_stats(&self, parameters: &[FieldDef]) {
    self.context.record_method();

    for param in parameters {
      if matches!(param.parameter_location, Some(ParameterLocation::Header))
        && let Some(original_name) = param.original_name.as_deref()
      {
        self.context.record_header(original_name);
      }
    }
  }

  fn collect_types(body_info: &BodyInfo, request_types: Vec<RustType>, response_types: Vec<RustType>) -> Vec<RustType> {
    body_info
      .generated_types
      .iter()
      .cloned()
      .chain(request_types)
      .chain(response_types)
      .collect::<Vec<_>>()
  }

  #[allow(clippy::too_many_arguments)]
  fn operation_info(
    &self,
    entry: &OperationEntry,
    base_name: &str,
    request_type: Option<StructToken>,
    response_enum: Option<EnumToken>,
    body_info: &BodyInfo,
    warnings: Vec<String>,
    parameters: Vec<FieldDef>,
  ) -> anyhow::Result<OperationInfo> {
    let response_metadata = self.response_converter.extract_metadata(&entry.operation);
    self.context.merge_usage(response_metadata.usage);

    Ok(
      OperationInfo::builder()
        .stable_id(&entry.stable_id)
        .operation_id(
          entry
            .operation
            .operation_id
            .clone()
            .unwrap_or_else(|| base_name.to_string()),
        )
        .method(entry.method.clone())
        .path(ParsedPath::parse(&entry.path, &parameters)?)
        .kind(entry.kind)
        .maybe_request_type(request_type)
        .maybe_response_type(response_metadata.metadata.type_name)
        .maybe_response_enum(response_enum)
        .response_media_types(response_metadata.metadata.media_types)
        .warnings(warnings)
        .parameters(parameters)
        .maybe_body(body_info.to_operation_body())
        .documentation(
          Documentation::documentation()
            .maybe_summary(entry.operation.summary.as_deref())
            .maybe_description(entry.operation.description.as_deref())
            .method(&entry.method)
            .path(&entry.path)
            .call(),
        )
        .build(),
    )
  }
}
