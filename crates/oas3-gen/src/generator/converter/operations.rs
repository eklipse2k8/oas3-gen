use std::rc::Rc;

use super::{
  ConverterContext, SchemaConverter, TypeUsageRecorder,
  requests::{BodyInfo, RequestConverter, RequestOutput},
  responses::ResponseConverter,
};
use crate::generator::{
  ast::{
    ContentCategory, Documentation, EnumToken, MultipartFieldInfo, OperationBody, OperationInfo, ParsedPath,
    ResponseEnumDef, RustPrimitive, RustType, StructToken,
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

/// Converts OpenAPI operations into Rust types and metadata.
///
/// Coordinates [`RequestConverter`] and [`ResponseConverter`] to transform
/// operation definitions into request/response type definitions.
#[derive(Debug, Clone)]
pub(crate) struct OperationConverter {
  context: Rc<ConverterContext>,
  schema_converter: SchemaConverter,
}

impl OperationConverter {
  /// Creates a new operation converter.
  pub(crate) fn new(context: Rc<ConverterContext>, schema_converter: SchemaConverter) -> Self {
    Self {
      context,
      schema_converter,
    }
  }

  /// Converts an OpenAPI operation into Rust types and metadata.
  ///
  /// The `stable_id` determines both the client method name (snake_case) and
  /// generated struct names (converted to PascalCase).
  pub(crate) fn convert(
    &self,
    entry: &OperationEntry,
    usage: &mut TypeUsageRecorder,
  ) -> anyhow::Result<ConversionResult> {
    let base_name = to_rust_type_name(&entry.stable_id);
    let mut types = vec![];
    let mut warnings = vec![];

    let body_info = BodyInfo::new(&self.context, entry)?;
    types.extend(body_info.generated_types.clone());
    usage.mark_request_iter(&body_info.type_usage);

    let response_converter = ResponseConverter::new(self.context.clone());
    let response_name = generate_unique_response_name(&base_name, |n| self.schema_converter.contains(n));
    let mut response_enum = response_converter.build_enum(&response_name, &entry.operation, &entry.path);

    let parse_response_method = response_enum
      .as_ref()
      .map(|def| ResponseConverter::build_parse_method(&EnumToken::new(def.name.to_string()), &def.variants));

    let request_name = generate_unique_request_name(&base_name, |n| self.schema_converter.contains(n));
    let request_output =
      RequestConverter::new(&self.context).build(&request_name, entry, &body_info, parse_response_method)?;

    warnings.extend(request_output.warnings.clone());
    let parameters = request_output.parameter_fields.clone();

    let request_type = Self::emit_request(&mut types, request_output, response_enum.is_some(), usage);

    if let Some(def) = response_enum.as_mut() {
      def.request_type.clone_from(&request_type);
    }

    let response_enum_token = Self::emit_response(&mut types, response_enum, usage);

    let response_metadata = response_converter.extract_metadata(&entry.operation, usage);

    let documentation = Documentation::documentation()
      .maybe_summary(entry.operation.summary.as_deref())
      .maybe_description(entry.operation.description.as_deref())
      .method(&entry.method)
      .path(&entry.path)
      .call();

    let operation_info = OperationInfo::builder()
      .stable_id(&entry.stable_id)
      .operation_id(
        entry
          .operation
          .operation_id
          .clone()
          .unwrap_or_else(|| base_name.clone()),
      )
      .method(entry.method.clone())
      .path(ParsedPath::parse(&entry.path, &parameters)?)
      .kind(entry.kind)
      .maybe_request_type(request_type)
      .maybe_response_type(response_metadata.type_name)
      .maybe_response_enum(response_enum_token)
      .response_media_types(response_metadata.media_types)
      .warnings(warnings)
      .parameters(parameters)
      .maybe_body(Self::extract_body(&body_info))
      .documentation(documentation)
      .build();

    Ok(ConversionResult { types, operation_info })
  }

  fn emit_request(
    types: &mut Vec<RustType>,
    output: RequestOutput,
    has_response: bool,
    usage: &mut TypeUsageRecorder,
  ) -> Option<StructToken> {
    let has_fields = !output.main_struct.fields.is_empty();
    if !has_fields && !has_response {
      return None;
    }

    types.extend(output.inline_types);
    types.extend(output.nested_structs.into_iter().map(RustType::Struct));

    let name = output.main_struct.name.clone();
    usage.mark_request(name.clone());
    types.push(RustType::Struct(output.main_struct));

    Some(name)
  }

  fn emit_response(
    types: &mut Vec<RustType>,
    response_enum: Option<ResponseEnumDef>,
    usage: &mut TypeUsageRecorder,
  ) -> Option<EnumToken> {
    let def = response_enum?;

    usage.mark_response(def.name.clone());
    for variant in &def.variants {
      if let Some(schema_type) = &variant.schema_type {
        usage.mark_response_type_ref(schema_type);
      }
    }

    let token = EnumToken::new(def.name.to_string());
    types.push(RustType::ResponseEnum(def));
    Some(token)
  }

  fn extract_body(body_info: &BodyInfo) -> Option<OperationBody> {
    let field_name = body_info.field_name.as_ref()?;
    let category = body_info
      .content_type
      .as_deref()
      .map_or(ContentCategory::Json, ContentCategory::from_content_type);

    let multipart_fields = if category == ContentCategory::Multipart {
      Self::resolve_multipart_fields(body_info)
    } else {
      None
    };

    Some(OperationBody {
      field_name: field_name.clone(),
      optional: body_info.optional,
      content_category: category,
      multipart_fields,
    })
  }

  fn resolve_multipart_fields(body_info: &BodyInfo) -> Option<Vec<MultipartFieldInfo>> {
    let body_type = body_info.body_type.as_ref()?;
    let body_type_name = body_type.unboxed_base_type_name();

    let struct_def = body_info.generated_types.iter().find_map(|t| {
      if let RustType::Struct(def) = t
        && def.name.as_str() == body_type_name
      {
        return Some(def);
      }
      None
    })?;

    let fields = struct_def
      .fields
      .iter()
      .map(|f| MultipartFieldInfo {
        name: f.name.clone(),
        nullable: f.rust_type.nullable,
        is_bytes: matches!(f.rust_type.base_type, RustPrimitive::Bytes),
        requires_json: f.rust_type.requires_json_serialization(),
      })
      .collect();

    Some(fields)
  }
}
