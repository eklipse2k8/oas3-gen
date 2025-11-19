pub(super) mod types;

use http::Method;
pub use types::{RustPrimitive, TypeRef};

/// Discriminated enum variant mapping
#[derive(Debug, Clone)]
pub struct DiscriminatedVariant {
  pub discriminator_value: String,
  pub variant_name: String,
  pub type_name: String,
}

/// Discriminated enum definition (uses macro for custom ser/de)
#[derive(Debug, Clone)]
pub struct DiscriminatedEnumDef {
  pub name: String,
  pub docs: Vec<String>,
  pub discriminator_field: String,
  pub variants: Vec<DiscriminatedVariant>,
  pub fallback: Option<DiscriminatedVariant>,
}

/// Response enum variant definition
#[derive(Debug, Clone)]
pub struct ResponseVariant {
  pub status_code: String,
  pub variant_name: String,
  pub description: Option<String>,
  pub schema_type: Option<TypeRef>,
  pub content_type: Option<String>,
}

/// Response enum definition for operation responses
#[derive(Debug, Clone)]
pub struct ResponseEnumDef {
  pub name: String,
  pub docs: Vec<String>,
  pub variants: Vec<ResponseVariant>,
  pub request_type: String,
}

/// Top-level Rust type representation
#[derive(Debug, Clone)]
pub enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
  DiscriminatedEnum(DiscriminatedEnumDef),
  ResponseEnum(ResponseEnumDef),
}

impl RustType {
  pub fn type_name(&self) -> &str {
    match self {
      RustType::Struct(def) => &def.name,
      RustType::Enum(def) => &def.name,
      RustType::TypeAlias(def) => &def.name,
      RustType::DiscriminatedEnum(def) => &def.name,
      RustType::ResponseEnum(def) => &def.name,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct OperationInfo {
  pub stable_id: String,
  pub operation_id: String,
  pub method: Method,
  pub path: String,
  pub summary: Option<String>,
  pub description: Option<String>,
  pub request_type: Option<String>,
  pub response_type: Option<String>,
  pub response_enum: Option<String>,
  pub response_content_type: Option<String>,
  pub request_body_types: Vec<String>,
  pub success_response_types: Vec<String>,
  pub error_response_types: Vec<String>,
  pub warnings: Vec<String>,
  pub parameters: Vec<OperationParameter>,
  pub body: Option<OperationBody>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterLocation {
  Path,
  Query,
  Header,
  Cookie,
}

#[derive(Debug, Clone)]
pub struct OperationParameter {
  pub original_name: String,
  pub rust_field: String,
  pub location: ParameterLocation,
  pub required: bool,
  pub rust_type: TypeRef,
}

#[derive(Debug, Clone)]
pub struct OperationBody {
  pub field_name: String,
  pub optional: bool,
  pub content_type: Option<String>,
}

/// Semantic kind of a struct to determine code generation behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructKind {
  /// Regular OpenAPI schema struct from components.schemas
  #[default]
  Schema,
  /// Operation request struct combining parameters and body (has `render_path` method)
  OperationRequest,
  /// Inline request body struct for an operation
  RequestBody,
}

/// Rust struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
  pub name: String,
  pub docs: Vec<String>,
  pub fields: Vec<FieldDef>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
  pub outer_attrs: Vec<String>,
  pub methods: Vec<StructMethod>,
  pub kind: StructKind,
}

/// Associated method definition for a struct
#[derive(Debug, Clone)]
pub struct StructMethod {
  pub name: String,
  pub docs: Vec<String>,
  pub kind: StructMethodKind,
  pub attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueryParameter {
  pub field: String,
  pub encoded_name: String,
  pub explode: bool,
  pub optional: bool,
  pub is_array: bool,
  pub style: Option<oas3::spec::ParameterStyle>,
}

#[derive(Debug, Clone)]
pub enum StructMethodKind {
  RenderPath {
    segments: Vec<PathSegment>,
    query_params: Vec<QueryParameter>,
  },
  ParseResponse {
    response_enum: String,
    variants: Vec<ResponseVariant>,
  },
}

#[derive(Debug, Clone)]
pub enum PathSegment {
  Literal(String),
  Parameter { field: String },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default)]
pub struct FieldDef {
  pub name: String,
  pub docs: Vec<String>,
  pub rust_type: TypeRef,
  pub serde_attrs: Vec<String>,
  pub extra_attrs: Vec<String>,
  pub validation_attrs: Vec<String>,
  pub regex_validation: Option<String>,
  pub default_value: Option<serde_json::Value>,
  pub example_value: Option<serde_json::Value>,
  pub parameter_location: Option<ParameterLocation>,
  pub read_only: bool,
  pub write_only: bool,
  pub deprecated: bool,
  pub multiple_of: Option<serde_json::Number>,
}

/// Rust enum definition
#[derive(Debug, Clone)]
pub struct EnumDef {
  pub name: String,
  pub docs: Vec<String>,
  pub variants: Vec<VariantDef>,
  pub discriminator: Option<String>,
  pub derives: Vec<String>,
  pub serde_attrs: Vec<String>,
  pub outer_attrs: Vec<String>,
}

/// Rust enum variant definition
#[derive(Debug, Clone)]
pub struct VariantDef {
  pub name: String,
  pub docs: Vec<String>,
  pub content: VariantContent,
  pub serde_attrs: Vec<String>,
  pub deprecated: bool,
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone)]
pub enum VariantContent {
  Unit,
  Tuple(Vec<TypeRef>),
}

/// Type alias definition
#[derive(Debug, Clone)]
pub struct TypeAliasDef {
  pub name: String,
  pub docs: Vec<String>,
  pub target: TypeRef,
}
