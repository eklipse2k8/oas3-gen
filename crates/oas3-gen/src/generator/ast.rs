/// Discriminated enum variant mapping
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatedVariant {
  pub(crate) discriminator_value: String,
  pub(crate) variant_name: String,
  pub(crate) type_name: String,
}

/// Discriminated enum definition (uses macro for custom ser/de)
#[derive(Debug, Clone)]
pub(crate) struct DiscriminatedEnumDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) discriminator_field: String,
  pub(crate) variants: Vec<DiscriminatedVariant>,
  pub(crate) fallback: Option<DiscriminatedVariant>,
}

/// Top-level Rust type representation
#[derive(Debug, Clone)]
pub(crate) enum RustType {
  Struct(StructDef),
  Enum(EnumDef),
  TypeAlias(TypeAliasDef),
  DiscriminatedEnum(DiscriminatedEnumDef),
}

impl RustType {
  pub(crate) fn type_name(&self) -> &str {
    match self {
      RustType::Struct(def) => &def.name,
      RustType::Enum(def) => &def.name,
      RustType::TypeAlias(def) => &def.name,
      RustType::DiscriminatedEnum(def) => &def.name,
    }
  }
}

/// Metadata about an API operation (for tracking, not direct code generation)
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct OperationInfo {
  pub(crate) operation_id: String,
  pub(crate) method: String,
  pub(crate) path: String,
  pub(crate) summary: Option<String>,
  pub(crate) description: Option<String>,
  pub(crate) request_type: Option<String>,
  pub(crate) response_type: Option<String>,
  pub(crate) request_body_types: Vec<String>,
}

/// Semantic kind of a struct to determine code generation behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StructKind {
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
pub(crate) struct StructDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) fields: Vec<FieldDef>,
  pub(crate) derives: Vec<String>,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) outer_attrs: Vec<String>,
  pub(crate) methods: Vec<StructMethod>,
  pub(crate) kind: StructKind,
}

/// Associated method definition for a struct
#[derive(Debug, Clone)]
pub(crate) struct StructMethod {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) kind: StructMethodKind,
  pub(crate) attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct QueryParameter {
  pub(crate) field: String,
  pub(crate) encoded_name: String,
  pub(crate) explode: bool,
  pub(crate) optional: bool,
  pub(crate) is_array: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum StructMethodKind {
  RenderPath {
    segments: Vec<PathSegment>,
    query_params: Vec<QueryParameter>,
  },
}

#[derive(Debug, Clone)]
pub(crate) enum PathSegment {
  Literal(String),
  Parameter { field: String },
}

/// Rust struct field definition
#[derive(Debug, Clone, Default)]
pub(crate) struct FieldDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) rust_type: TypeRef,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) extra_attrs: Vec<String>,
  pub(crate) validation_attrs: Vec<String>,
  pub(crate) regex_validation: Option<String>,
  pub(crate) default_value: Option<serde_json::Value>,
  pub(crate) read_only: bool,
  pub(crate) write_only: bool,
  pub(crate) deprecated: bool,
  pub(crate) multiple_of: Option<serde_json::Number>,
}

/// Type reference with wrapper support (Box, Option, Vec)
#[derive(Debug, Clone, Default)]
pub(crate) struct TypeRef {
  pub(crate) base_type: String,
  pub(crate) boxed: bool,
  pub(crate) nullable: bool,
  pub(crate) is_array: bool,
  pub(crate) unique_items: bool,
}

impl TypeRef {
  pub(crate) fn new(base_type: impl Into<String>) -> Self {
    Self {
      base_type: base_type.into(),
      boxed: false,
      nullable: false,
      is_array: false,
      unique_items: false,
    }
  }

  pub(crate) fn with_option(mut self) -> Self {
    self.nullable = true;
    self
  }

  pub(crate) fn with_vec(mut self) -> Self {
    self.is_array = true;
    self
  }

  pub(crate) fn with_unique_items(mut self, unique: bool) -> Self {
    self.unique_items = unique;
    self
  }

  pub(crate) fn with_boxed(mut self) -> Self {
    self.boxed = true;
    self
  }

  /// Get the full Rust type string
  pub(crate) fn to_rust_type(&self) -> String {
    let mut result = self.base_type.clone();

    if self.boxed {
      result = format!("Box<{result}>");
    }

    if self.is_array {
      result = format!("Vec<{result}>");
    }

    if self.nullable {
      result = format!("Option<{result}>");
    }

    result
  }
}

/// Rust enum definition
#[derive(Debug, Clone)]
pub(crate) struct EnumDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) variants: Vec<VariantDef>,
  pub(crate) discriminator: Option<String>,
  pub(crate) derives: Vec<String>,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) outer_attrs: Vec<String>,
}

/// Rust enum variant definition
#[derive(Debug, Clone)]
pub(crate) struct VariantDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) content: VariantContent,
  pub(crate) serde_attrs: Vec<String>,
  pub(crate) deprecated: bool,
}

/// Enum variant content (Unit, Tuple, or Struct)
#[derive(Debug, Clone)]
pub(crate) enum VariantContent {
  Unit,
  Tuple(Vec<TypeRef>),
  Struct(Vec<FieldDef>),
}

/// Type alias definition
#[derive(Debug, Clone)]
pub(crate) struct TypeAliasDef {
  pub(crate) name: String,
  pub(crate) docs: Vec<String>,
  pub(crate) target: TypeRef,
}
