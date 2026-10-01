use std::rc::Rc;

use indexmap::IndexSet;
use oas3::spec::ParameterIn;

use super::{
  ConverterContext, GenerationTarget, SchemaConverter, SerdeUsageRecorder,
  requests::{BodyInfo, RequestConverter, RequestOutput},
  responses::ResponseConverter,
  security::{SecurityConverter, credentials_structs},
};
use crate::{
  generator::{
    ast::{
      Documentation, FieldDef, OperationInfo, ParsedPath, ResponseVariant, RustType, StructToken,
      constants::HttpHeaderRef,
    },
    metrics::GenerationWarning,
    naming::{identifiers::to_rust_type_name, operations::generate_unique_request_name},
    operation_registry::OperationEntry,
  },
  utils::schema_ext::SchemaExtIters,
};

/// Result of converting a single OpenAPI operation.
///
/// Contains generated types (request struct, response enum, inline types)
/// and metadata about the operation (path, method, parameters).
#[derive(Debug, Clone)]
pub(crate) struct ConversionResult {
  pub(crate) types: Vec<RustType>,
  pub(crate) operation_info: OperationInfo,
}

/// Aggregate result of converting all operations in a specification.
///
/// Collects types, operation metadata, warnings, and type usage data
/// from the full conversion pass.
#[derive(Debug, Clone)]
pub(crate) struct OperationsOutput {
  pub(crate) types: Vec<RustType>,
  pub(crate) operations: Vec<OperationInfo>,
  pub(crate) warnings: Vec<GenerationWarning>,
  pub(crate) usage_recorder: SerdeUsageRecorder,
  pub(crate) unique_headers: IndexSet<HttpHeaderRef>,
}

/// Orchestrates conversion of all operations in a specification.
///
/// Iterates over operation entries, converts each to request/response
/// types, and aggregates results with error handling and warning collection.
#[derive(Debug, Clone)]
pub(crate) struct OperationsProcessor {
  context: Rc<ConverterContext>,
  converter: OperationConverter,
}

impl OperationsProcessor {
  /// Creates a new operations processor with the shared converter context.
  pub(crate) fn new(context: Rc<ConverterContext>, schema_converter: &SchemaConverter) -> Self {
    let converter = OperationConverter::new(context.clone(), schema_converter.clone());
    Self { context, converter }
  }

  /// Converts all operations, collecting types and warnings.
  ///
  /// Operations that fail to convert emit warnings rather than failing
  /// the entire generation. Returns accumulated type usage data for
  /// serde derive optimization in postprocessing.
  pub(crate) fn process_all<'a>(&self, entries: impl Iterator<Item = &'a OperationEntry>) -> OperationsOutput {
    let entries = entries.collect::<Vec<_>>();
    let mut types = vec![];
    let mut operations = vec![];
    let mut warnings = self
      .converter
      .security
      .unsupported_warnings(entries.iter().map(|entry| entry.operation.as_ref()));
    let mut unique_headers = IndexSet::new();

    for entry in entries {
      match self.converter.convert(entry) {
        Ok(ConversionResult {
          types: operation_types,
          operation_info,
        }) => {
          warnings.extend(operation_info.warnings());
          unique_headers.extend(operation_info.header_names());
          types.extend(operation_types);
          operations.push(operation_info);
        }
        Err(error) => warnings.push(GenerationWarning::conversion_failure(entry, &error)),
      }
    }

    types.extend(credentials_structs(&operations));

    if self.context.config().include_all_headers() {
      self.extend_component_headers(&mut unique_headers);
    }

    OperationsOutput {
      types,
      operations,
      warnings,
      usage_recorder: self.context.take_type_usage(),
      unique_headers,
    }
  }

  fn extend_component_headers(&self, unique_headers: &mut IndexSet<HttpHeaderRef>) {
    let spec = self.context.graph().spec();
    unique_headers.extend(
      spec
        .components
        .iter()
        .flat_map(|components| components.parameters.values())
        .resolve_all(spec)
        .filter(|parameter| parameter.location == ParameterIn::Header)
        .map(|parameter| HttpHeaderRef::from(&parameter.name)),
    );
  }
}

type RequestTypes = (Vec<RustType>, Option<StructToken>);

/// Converts OpenAPI operations into Rust types and metadata.
///
/// Coordinates [`RequestConverter`] and [`ResponseConverter`] to transform
/// operation definitions into request types and response variants.
#[derive(Debug, Clone)]
pub(crate) struct OperationConverter {
  context: Rc<ConverterContext>,
  schema_converter: SchemaConverter,
  response_converter: ResponseConverter,
  request_converter: RequestConverter,
  security: SecurityConverter,
}

impl OperationConverter {
  /// Creates a new operation converter with request and response sub-converters.
  pub(crate) fn new(context: Rc<ConverterContext>, schema_converter: SchemaConverter) -> Self {
    let response_converter = ResponseConverter::new(context.clone());
    let request_converter = RequestConverter::new(&context);
    let security = SecurityConverter::new(&context);

    Self {
      context,
      schema_converter,
      response_converter,
      request_converter,
      security,
    }
  }

  /// Converts a single operation entry into types and metadata.
  ///
  /// Generates the request struct (with parameters and body), collects the
  /// declared response variants, and records operation metadata for
  /// client/server code generation.
  pub(crate) fn convert(&self, entry: &OperationEntry) -> anyhow::Result<ConversionResult> {
    let base_name = to_rust_type_name(&entry.stable_id);
    let body_info = BodyInfo::new(&self.context, entry)?;

    self.context.mark_request_iter(&body_info.type_usage);

    let response_variants = self
      .response_converter
      .build_variants(&entry.operation, &entry.path, &base_name)?;
    let security = self.security.convert(&entry.operation);
    let credentials_type = security
      .as_ref()
      .filter(|_| self.context.config().target == GenerationTarget::Server)
      .map(|security| self.security.credentials_type(security));
    let request_output = self.request(&base_name, entry, &body_info, credentials_type.as_ref())?;

    let warnings = request_output.warnings.clone();
    let parameters = request_output.parameter_fields.clone();

    let (request_types, request_type) = self.request_types(request_output, response_variants.is_some());
    self.mark_response_types(response_variants.as_deref().unwrap_or_default());

    let types = Self::collect_types(&body_info, request_types);

    let mut operation_info = self.operation_info(
      entry,
      &base_name,
      request_type,
      response_variants,
      &body_info,
      warnings,
      parameters,
    )?;
    operation_info.security = security;
    operation_info.credentials_type = credentials_type;

    Ok(ConversionResult { types, operation_info })
  }

  /// Builds the request struct with parameters, credentials, body, and methods.
  fn request(
    &self,
    base_name: &str,
    entry: &OperationEntry,
    body_info: &BodyInfo,
    credentials_type: Option<&StructToken>,
  ) -> anyhow::Result<RequestOutput> {
    let request_name = generate_unique_request_name(base_name, |n| self.schema_converter.contains(n));
    self
      .request_converter
      .build(&request_name, entry, body_info, credentials_type)
  }

  /// Assembles request types and marks them as request-context types.
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

  /// Marks every response body type as a response-context type.
  fn mark_response_types(&self, variants: &[ResponseVariant]) {
    for schema_type in variants.iter().filter_map(|v| v.schema_type.as_ref()) {
      self.context.mark_response_type_ref(schema_type);
    }
  }

  /// Combines body and request types into a single collection.
  fn collect_types(body_info: &BodyInfo, request_types: Vec<RustType>) -> Vec<RustType> {
    body_info
      .generated_types
      .iter()
      .cloned()
      .chain(request_types)
      .collect::<Vec<_>>()
  }

  /// Builds the operation metadata for client/server code generation.
  #[allow(clippy::too_many_arguments)]
  fn operation_info(
    &self,
    entry: &OperationEntry,
    base_name: &str,
    request_type: Option<StructToken>,
    response_variants: Option<Vec<ResponseVariant>>,
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
        .maybe_response_variants(response_variants)
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
