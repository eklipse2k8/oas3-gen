use std::{collections::BTreeSet, rc::Rc};

use super::{
  ConverterContext, SchemaConverter, SerdeUsageRecorder,
  requests::{BodyInfo, RequestConverter, RequestOutput},
  responses::ResponseConverter,
};
use crate::generator::{
  ast::{
    Documentation, EnumToken, FieldDef, HandlerBodyInfo, MethodNameToken, OperationInfo, ParameterLocation, ParsedPath,
    ResponseEnumDef, RustType, ServerRequestTraitDef, ServerTraitMethod, StructMethod, StructToken, TraitToken,
  },
  metrics::GenerationWarning,
  naming::{
    identifiers::to_rust_type_name,
    operations::{generate_unique_request_name, generate_unique_response_name},
  },
  operation_registry::OperationEntry,
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
  pub(crate) unique_headers: BTreeSet<String>,
}

/// Orchestrates conversion of all operations in a specification.
///
/// Iterates over operation entries, converts each to request/response
/// types, and aggregates results with error handling and warning collection.
pub(crate) struct OperationsProcessor {
  context: Rc<ConverterContext>,
  schema_converter: SchemaConverter,
}

impl OperationsProcessor {
  /// Creates a new operations processor with the shared converter context.
  pub(crate) fn new(context: Rc<ConverterContext>, schema_converter: SchemaConverter) -> Self {
    Self {
      context,
      schema_converter,
    }
  }

  /// Converts all operations, collecting types and warnings.
  ///
  /// Operations that fail to convert emit warnings rather than failing
  /// the entire generation. Returns accumulated type usage data for
  /// serde derive optimization in postprocessing.
  pub(crate) fn process_all<'a>(&self, entries: impl Iterator<Item = &'a OperationEntry>) -> OperationsOutput {
    let mut rust_types = vec![];
    let mut operations_info = vec![];
    let mut warnings = vec![];
    let mut unique_headers = BTreeSet::new();

    let operation_converter = OperationConverter::new(self.context.clone(), self.schema_converter.clone());

    for entry in entries {
      match operation_converter.convert(entry) {
        Ok(result) => {
          warnings.extend(
            result
              .operation_info
              .warnings
              .iter()
              .map(|w| GenerationWarning::OperationSpecific {
                operation_id: result.operation_info.operation_id.clone(),
                message: w.clone(),
              }),
          );

          Self::collect_header_names(&result.operation_info.parameters, &mut unique_headers);

          rust_types.extend(result.types);
          operations_info.push(result.operation_info);
        }
        Err(e) => {
          warnings.push(GenerationWarning::OperationConversionFailed {
            method: entry.method.to_string(),
            path: entry.path.clone(),
            error: e.to_string(),
          });
        }
      }
    }

    OperationsOutput {
      types: rust_types,
      operations: operations_info,
      warnings,
      usage_recorder: self.context.take_type_usage(),
      unique_headers,
    }
  }

  fn collect_header_names(parameters: &[FieldDef], headers: &mut BTreeSet<String>) {
    for param in parameters {
      if matches!(param.parameter_location, Some(ParameterLocation::Header))
        && let Some(original_name) = param.original_name.as_deref()
      {
        headers.insert(original_name.to_ascii_lowercase());
      }
    }
  }
}

/// Builds the server trait definition from converted operations.
///
/// Creates an `ApiServer` trait with one method per operation, including
/// typed path, query, and header parameter structs. Returns `None` if
/// there are no operations to include.
pub(crate) fn build_server_trait(operations: &[OperationInfo]) -> Option<ServerRequestTraitDef> {
  if operations.is_empty() {
    return None;
  }

  let methods = operations
    .iter()
    .map(|info| {
      let path_params_type = extract_nested_type(&info.parameters, ParameterLocation::Path, info.request_type.as_ref());
      let query_params_type =
        extract_nested_type(&info.parameters, ParameterLocation::Query, info.request_type.as_ref());
      let header_params_type =
        extract_nested_type(&info.parameters, ParameterLocation::Header, info.request_type.as_ref());

      let body_info = info.body.as_ref().and_then(|body| {
        body.body_type.as_ref().map(|body_type| {
          HandlerBodyInfo::builder()
            .body_type(body_type.clone())
            .content_category(body.content_category)
            .optional(body.optional)
            .build()
        })
      });

      ServerTraitMethod::builder()
        .name(MethodNameToken::from_raw(&info.stable_id))
        .docs(info.documentation.clone())
        .maybe_request_type(info.request_type.clone())
        .maybe_response_type(info.response_enum.clone())
        .http_method(info.method.clone())
        .path(info.path.clone())
        .maybe_path_params_type(path_params_type)
        .maybe_query_params_type(query_params_type)
        .maybe_header_params_type(header_params_type)
        .maybe_body_info(body_info)
        .build()
    })
    .collect::<Vec<_>>();

  Some(
    ServerRequestTraitDef::builder()
      .name(TraitToken::new("ApiServer"))
      .methods(methods)
      .build(),
  )
}

/// Extracts the nested parameter struct type for a specific location.
///
/// Returns the struct name (e.g., `GetUsersRequestPath`) if any parameters
/// exist for the given location, `None` otherwise.
fn extract_nested_type(
  parameters: &[FieldDef],
  location: ParameterLocation,
  request_type: Option<&StructToken>,
) -> Option<StructToken> {
  let has_params = parameters.iter().any(|p| p.parameter_location == Some(location));
  let suffix = location.suffix()?;

  has_params
    .then(|| request_type.map(|req| StructToken::new(format!("{req}{suffix}"))))
    .flatten()
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
  /// Creates a new operation converter with request and response sub-converters.
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

  /// Converts a single operation entry into types and metadata.
  ///
  /// Generates request struct (with parameters and body), response enum,
  /// and collects operation metadata for client/server code generation.
  pub(crate) fn convert(&self, entry: &OperationEntry) -> anyhow::Result<ConversionResult> {
    let base_name = to_rust_type_name(&entry.stable_id);
    let body_info = BodyInfo::new(&self.context, entry)?;

    self.context.mark_request_iter(&body_info.type_usage);

    let (response_def, parse_method) = self.response_definition(&base_name, entry);
    let request_output = self.request(&base_name, entry, &body_info, parse_method)?;

    let warnings = request_output.warnings.clone();
    let parameters = request_output.parameter_fields.clone();

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

  /// Builds the response enum and parse method for an operation.
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

  /// Builds the request struct with parameters, body, and methods.
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

  /// Assembles response types and marks them as response-context types.
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

  /// Combines body, request, and response types into a single collection.
  fn collect_types(body_info: &BodyInfo, request_types: Vec<RustType>, response_types: Vec<RustType>) -> Vec<RustType> {
    body_info
      .generated_types
      .iter()
      .cloned()
      .chain(request_types)
      .chain(response_types)
      .collect::<Vec<_>>()
  }

  /// Builds the operation metadata for client/server code generation.
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
