/// Re-exported from `better_default` to enable `#[default(value)]` attribute on struct fields.
pub use better_default::Default;
/// Re-exported from `http` for HTTP method types in generated request structs.
pub use http::Method;
/// Re-exported from `percent_encoding` to support custom URL encoding sets.
pub use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
/// Re-exported from `serde_with` to enable `#[skip_serializing_none]` on generated types.
pub use serde_with::skip_serializing_none;

#[derive(Debug, thiserror::Error)]
pub enum JsonDiagnostics {
  #[error("Failed to read response body: {0}")]
  BodyReadError(#[from] reqwest::Error),
  #[error("Deserialization error: {inner}, path={path}")]
  DeserializationError { path: String, inner: serde_json::Error },
}

#[allow(async_fn_in_trait)]
pub trait Diagnostics<T>
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, JsonDiagnostics>;
}

impl<T> Diagnostics<T> for reqwest::Response
where
  T: serde::de::DeserializeOwned,
{
  async fn json_with_diagnostics(self) -> Result<T, JsonDiagnostics> {
    let raw_body = self.text().await.map_err(JsonDiagnostics::BodyReadError)?;
    let mut de = serde_json::Deserializer::from_str(&raw_body);
    serde_path_to_error::deserialize(&mut de).map_err(|err| JsonDiagnostics::DeserializationError {
      path: err.path().to_string(),
      inner: err.into_inner(),
    })
  }
}

#[doc(hidden)]
#[macro_export]
macro_rules! discriminated_enum_default_helper {
  ($fallback_type:ty, $constructor:expr) => {
    $constructor(<$fallback_type>::default())
  };
}

/// Generates a Rust enum with discriminator-based serialization and deserialization.
///
/// This macro creates an enum that uses a specific field value (discriminator) to determine
/// which variant to deserialize into. This pattern is commonly used for `OpenAPI` `oneOf` and
/// `anyOf` schemas with a discriminator property, enabling type-safe handling of polymorphic
/// JSON responses.
///
/// The generated enum automatically implements `Serialize`, `Deserialize`, `Debug`, `Clone`,
/// and `PartialEq`. During deserialization, the discriminator field value determines which
/// variant to instantiate. Each variant wraps a struct type that contains the discriminator
/// field along with variant-specific fields.
///
/// # Syntax
///
/// Two forms are supported:
///
/// **With fallback variant:**
/// ```ignore
/// discriminated_enum! {
///   pub enum MyEnum {
///     discriminator: "type_field",
///     variants: [
///       ("variant_a", VariantA(VariantAType)),
///       ("variant_b", VariantB(VariantBType)),
///     ],
///     fallback: Default(DefaultType),
///   }
/// }
/// ```
///
/// **Without fallback (strict matching):**
/// ```ignore
/// discriminated_enum! {
///   pub enum MyEnum {
///     discriminator: "type_field",
///     variants: [
///       ("variant_a", VariantA(VariantAType)),
///       ("variant_b", VariantB(VariantBType)),
///     ],
///   }
/// }
/// ```
///
/// # Parameters
///
/// - `discriminator`: The JSON field name used to determine the variant type
/// - `variants`: Array of tuples `(discriminator_value, VariantName(VariantType))`
/// - `fallback`: (Optional) A catch-all variant used when the discriminator is missing
///
/// # Behavior
///
/// **With fallback:**
/// - Matched discriminator value: Deserializes into the corresponding variant
/// - Missing discriminator field: Uses fallback variant
/// - Unknown discriminator value: Returns deserialization error
///
/// **Without fallback:**
/// - Matched discriminator value: Deserializes into the corresponding variant
/// - Missing discriminator field: Returns missing field error
/// - Unknown discriminator value: Returns deserialization error
///
/// # Type Support
///
/// Variant types can be:
/// - Plain structs: `VariantA(StructType)`
/// - Boxed types: `VariantA(Box<StructType>)` (enables recursive/cyclic types)
///
/// # Examples
///
/// ```
/// use oas3_gen_support::{Default, discriminated_enum};
///
/// #[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
/// #[serde(default)]
/// struct Circle {
///   #[default("circle".to_string())]
///   shape_type: String,
///   radius: f64,
/// }
///
/// #[derive(Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
/// #[serde(default)]
/// struct Rectangle {
///   #[default("rectangle".to_string())]
///   shape_type: String,
///   width: f64,
///   height: f64,
/// }
///
/// discriminated_enum! {
///   pub enum Shape {
///     discriminator: "shape_type",
///     variants: [
///       ("circle", Circle(Circle)),
///       ("rectangle", Rectangle(Rectangle)),
///     ],
///   }
/// }
///
/// let json = r#"{"shape_type": "circle", "radius": 5.0}"#;
/// let shape: Shape = serde_json::from_str(json).unwrap();
/// assert!(matches!(shape, Shape::Circle(_)));
/// ```
///
/// # Generated Code
///
/// The macro generates:
/// - Enum definition with specified variants
/// - `DISCRIMINATOR_FIELD` constant for introspection
/// - `Default` implementation (uses fallback variant or first variant)
/// - `Serialize` implementation (delegates to inner type)
/// - `Deserialize` implementation (discriminator-based routing)
#[macro_export]
macro_rules! discriminated_enum {
  (
    $(#[$meta:meta])*
    $vis:vis enum $name:ident {
      discriminator: $disc_field:expr,
      variants: [
        $(($disc_value:expr, $variant:ident($variant_type:ty))),* $(,)?
      ],
      fallback: $fallback_variant:ident($fallback_type:ty) $(,)?
    }
  ) => {
    $(#[$meta])*
    #[derive(Debug, Clone, PartialEq)]
    $vis enum $name {
      $($variant($variant_type),)*
      $fallback_variant($fallback_type),
    }

    impl $name {
      $vis const DISCRIMINATOR_FIELD: &'static str = $disc_field;
    }

    impl Default for $name {
      fn default() -> Self {
        $crate::discriminated_enum_default_helper!($fallback_type, Self::$fallback_variant)
      }
    }

    impl serde::Serialize for $name {
      fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
      where
        S: serde::Serializer,
      {
        match self {
          $(Self::$variant(v) => v.serialize(serializer),)*
          Self::$fallback_variant(v) => v.serialize(serializer),
        }
      }
    }

    impl<'de> serde::Deserialize<'de> for $name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get(Self::DISCRIMINATOR_FIELD).and_then(|v| v.as_str()) {
          $(
            Some($disc_value) =>
              serde_json::from_value(value)
                .map(Self::$variant)
                .map_err(serde::de::Error::custom),
          )*
          None => {
            serde_json::from_value(value)
              .map(Self::$fallback_variant)
              .map_err(serde::de::Error::custom)
          }
          Some(other) => Err(serde::de::Error::custom(format!(
            "Unknown discriminator value '{}' for field '{}'",
            other, Self::DISCRIMINATOR_FIELD
          ))),
        }
      }
    }
  };

  (
    $(#[$meta:meta])*
    $vis:vis enum $name:ident {
      discriminator: $disc_field:expr,
      variants: [
        $(($disc_value:expr, $variant:ident($variant_type:ty))),* $(,)?
      ] $(,)?
    }
  ) => {
    $(#[$meta])*
    #[derive(Debug, Clone, PartialEq)]
    $vis enum $name {
      $($variant($variant_type),)*
    }

    impl $name {
      $vis const DISCRIMINATOR_FIELD: &'static str = $disc_field;
    }

    impl serde::Serialize for $name {
      fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
      where
        S: serde::Serializer,
      {
        match self {
          $(Self::$variant(v) => v.serialize(serializer),)*
        }
      }
    }

    impl<'de> serde::Deserialize<'de> for $name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: serde::Deserializer<'de>,
      {
        let value = serde_json::Value::deserialize(deserializer)?;

        match value.get(Self::DISCRIMINATOR_FIELD).and_then(|v| v.as_str()) {
          $(
            Some($disc_value) =>
              serde_json::from_value(value)
                .map(Self::$variant)
                .map_err(serde::de::Error::custom),
          )*
          None => Err(serde::de::Error::missing_field(Self::DISCRIMINATOR_FIELD)),
          Some(other) => Err(serde::de::Error::custom(format!(
            "Unknown discriminator value '{}' for field '{}'",
            other, Self::DISCRIMINATOR_FIELD
          ))),
        }
      }
    }
  };
}

/// Character set for percent-encoding URL path segments according to RFC 3986.
///
/// This set preserves unreserved characters (`-`, `_`, `.`, `~`) and encodes all other
/// non-alphanumeric characters. Used by [`percent_encode_path_segment`] to safely encode
/// dynamic path parameters in HTTP request URLs.
///
/// # RFC 3986 Compliance
///
/// Unreserved characters that are NOT encoded: `A-Z a-z 0-9 - _ . ~`
///
/// All other characters are percent-encoded, including reserved characters like `/` which
/// have special meaning in URL paths and must be encoded when appearing in path parameter
/// values.
pub const PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Percent-encodes a URL path segment according to RFC 3986 rules.
///
/// This function encodes a string value for safe inclusion as a path parameter in an HTTP
/// request URL. It preserves unreserved characters and encodes everything else, including
/// path delimiters like `/` that would otherwise be interpreted as path separators.
///
/// Generated code calls this function when constructing request URLs with dynamic path
/// parameters, such as `/users/{user_id}/posts/{post_id}`.
///
/// # RFC 3986 Compliance
///
/// Follows RFC 3986 path encoding rules:
/// - Unreserved characters (`A-Z a-z 0-9 - _ . ~`) are preserved
/// - All other characters, including spaces and `/`, are percent-encoded
/// - For example: `hello/world` → `hello%2Fworld`
///
/// # Examples
///
/// ```
/// use oas3_gen_support::percent_encode_path_segment;
///
/// assert_eq!(percent_encode_path_segment("user123"), "user123");
/// assert_eq!(percent_encode_path_segment("hello world"), "hello%20world");
/// assert_eq!(percent_encode_path_segment("path/with/slashes"), "path%2Fwith%2Fslashes");
/// assert_eq!(percent_encode_path_segment("user@example.com"), "user%40example.com");
/// ```
///
/// # Performance
///
/// This function is marked `#[inline]` for performance. It only allocates when encoding
/// is required; if the input contains only unreserved characters, it returns a clone.
#[inline]
#[must_use]
pub fn percent_encode_path_segment(segment: &str) -> String {
  utf8_percent_encode(segment, PATH_ENCODE_SET).to_string()
}

/// Character set for percent-encoding URL query string components according to RFC 3986.
///
/// This set preserves unreserved characters (`-`, `_`, `.`, `~`) and encodes all other
/// non-alphanumeric characters. Used by [`percent_encode_query_component`] to safely encode
/// query parameter values in HTTP request URLs.
///
/// # RFC 3986 Compliance
///
/// Unreserved characters that are NOT encoded: `A-Z a-z 0-9 - _ . ~`
///
/// All other characters are percent-encoded, including query-significant characters like
/// `&`, `=`, and `+` which have special meaning in query strings and must be encoded when
/// appearing in parameter values.
pub const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

/// Percent-encodes a URL query parameter value according to RFC 3986 rules.
///
/// This function encodes a string value for safe inclusion as a query parameter value in
/// an HTTP request URL. It preserves unreserved characters and encodes everything else,
/// including query delimiters like `&` and `=` that would otherwise interfere with query
/// string parsing.
///
/// Generated code calls this function when constructing request URLs with query parameters,
/// typically after serializing the parameter value with [`serialize_query_param`]. The
/// combination handles both type conversion (Rust value → string) and URL safety (string
/// → percent-encoded string).
///
/// # RFC 3986 Compliance
///
/// Follows RFC 3986 query encoding rules:
/// - Unreserved characters (`A-Z a-z 0-9 - _ . ~`) are preserved
/// - All other characters, including spaces, `&`, `=`, and `+`, are percent-encoded
/// - For example: `hello world` → `hello%20world`
/// - For example: `a+b=c&d` → `a%2Bb%3Dc%26d`
///
/// # Examples
///
/// ```
/// use oas3_gen_support::percent_encode_query_component;
///
/// assert_eq!(percent_encode_query_component("simple"), "simple");
/// assert_eq!(percent_encode_query_component("hello world"), "hello%20world");
/// assert_eq!(percent_encode_query_component("a+b"), "a%2Bb");
/// assert_eq!(percent_encode_query_component("user@example.com"), "user%40example.com");
/// assert_eq!(percent_encode_query_component("a=b&c=d"), "a%3Db%26c%3Dd");
/// ```
///
/// # Performance
///
/// This function is marked `#[inline]` for performance. It only allocates when encoding
/// is required; if the input contains only unreserved characters, it returns a clone.
#[inline]
#[must_use]
pub fn percent_encode_query_component(component: &str) -> String {
  utf8_percent_encode(component, QUERY_ENCODE_SET).to_string()
}

/// Serializes a Rust value to a plain string representation for use in URL query parameters.
///
/// This function converts Rust values (primitives, enums, Option types) into their string
/// representations suitable for URL query parameters. It uses `serde_plain` for serialization,
/// which handles common types including:
/// - Primitives: `42` → `"42"`, `true` → `"true"`, `3.14` → `"3.14"`
/// - Strings: `"hello"` → `"hello"`
/// - Enums: Serializes using serde rename attributes (e.g., `#[serde(rename = "value_1")]`)
/// - Options: `Some(42)` → `"42"`, `None` → `""`
///
/// Generated code calls this function before [`percent_encode_query_component`] to build
/// complete query strings. The two-step process (serialize → encode) ensures both type safety
/// and URL safety.
///
/// # Examples
///
/// ```
/// use oas3_gen_support::serialize_query_param;
///
/// assert_eq!(serialize_query_param(&42).unwrap(), "42");
/// assert_eq!(serialize_query_param(&true).unwrap(), "true");
/// assert_eq!(serialize_query_param(&"hello").unwrap(), "hello");
/// assert_eq!(serialize_query_param(&Some(42)).unwrap(), "42");
/// assert_eq!(serialize_query_param(&None::<i32>).unwrap(), "");
/// ```
///
/// # Enum Serialization
///
/// For enums with `#[serde(rename = "...")]` attributes, the renamed value is used:
///
/// ```ignore
/// #[derive(serde::Serialize)]
/// enum Status {
///   #[serde(rename = "active")]
///   Active,
///   #[serde(rename = "inactive")]
///   Inactive,
/// }
///
/// assert_eq!(serialize_query_param(&Status::Active).unwrap(), "active");
/// ```
///
/// # Errors
///
/// Returns `Err` if the value cannot be serialized to a plain string representation.
/// This can occur for:
/// - Complex nested structures (maps, nested objects)
/// - Types without appropriate `Serialize` implementations
/// - Custom serialization logic that produces non-plain-text output
///
/// In practice, errors are rare for the primitive and enum types typically used in query
/// parameters. Generated code propagates errors using `?` to the caller.
#[inline]
pub fn serialize_query_param<T: serde::Serialize>(value: &T) -> Result<String, serde_plain::Error> {
  serde_plain::to_string(value)
}

#[cfg(test)]
mod tests {

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct MappingAType {
    #[default("a".to_string())]
    discrim: String,
    value: i32,
    #[serde(flatten)]
    parent: ParentType,
  }

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct MappingBType {
    #[default("b".to_string())]
    discrim: String,
    final_result: i32,
    #[serde(flatten)]
    mapping_a: MappingAType,
  }

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct ParentType {
    id: i32,
  }

  discriminated_enum! {
    enum TestEnum {
      discriminator: "discrim",
      variants: [
        ("a", MappingA(MappingAType)),
        ("b", MappingB(MappingBType)),
      ],
      fallback: Parent(ParentType),
    }
  }

  #[test]
  fn test_discriminated_enum() {
    let json = r#"{"discrim":"a","id":999,"value":42}"#;
    let deserialized: TestEnum = serde_json::from_str(json).unwrap();
    let expected = TestEnum::MappingA(MappingAType {
      value: 42,
      parent: ParentType { id: 999 },
      ..Default::default()
    });

    assert_eq!(deserialized, expected);
  }

  #[test]
  fn test_mid_discriminated_enum() {
    let json = r#"{"discrim":"b","id":999,"final_result":42}"#;
    let deserialized: TestEnum = serde_json::from_str(json).unwrap();
    let expected = TestEnum::MappingB(MappingBType {
      final_result: 42,
      mapping_a: MappingAType {
        parent: ParentType { id: 999 },
        ..Default::default()
      },
      ..Default::default()
    });

    assert_eq!(deserialized, expected);
  }

  #[test]
  fn test_discriminated_enum_fallback() {
    let json = r#"{"id":123}"#;
    let deserialized: TestEnum = serde_json::from_str(json).unwrap();
    let expected = TestEnum::Parent(ParentType { id: 123 });

    assert_eq!(deserialized, expected);
  }

  #[test]
  fn test_discriminated_enum_default() {
    let default_value = TestEnum::default();
    let expected = TestEnum::Parent(ParentType::default());

    assert_eq!(default_value, expected);
  }

  // Tests for Box variant support

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct BoxedTypeA {
    #[default("boxed_a".to_string())]
    discrim: String,
    data: String,
  }

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct BoxedTypeB {
    #[default("boxed_b".to_string())]
    discrim: String,
    count: i32,
  }

  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct BoxedFallback {
    id: i32,
  }

  discriminated_enum! {
    enum BoxedEnum {
      discriminator: "discrim",
      variants: [
        ("boxed_a", VariantA(Box<BoxedTypeA>)),
        ("boxed_b", VariantB(Box<BoxedTypeB>)),
      ],
      fallback: Fallback(Box<BoxedFallback>),
    }
  }

  #[test]
  fn test_boxed_variant_deserialization() {
    let json = r#"{"discrim":"boxed_a","data":"test"}"#;
    let deserialized: BoxedEnum = serde_json::from_str(json).unwrap();
    let expected = BoxedEnum::VariantA(Box::new(BoxedTypeA {
      discrim: "boxed_a".to_string(),
      data: "test".to_string(),
    }));

    assert_eq!(deserialized, expected);
  }

  #[test]
  fn test_boxed_variant_serialization() {
    let value = BoxedEnum::VariantB(Box::new(BoxedTypeB {
      discrim: "boxed_b".to_string(),
      count: 42,
    }));
    let json = serde_json::to_string(&value).unwrap();
    let deserialized: BoxedEnum = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized, value);
  }

  #[test]
  fn test_boxed_fallback() {
    let json = r#"{"id":999}"#;
    let deserialized: BoxedEnum = serde_json::from_str(json).unwrap();
    let expected = BoxedEnum::Fallback(Box::new(BoxedFallback { id: 999 }));

    assert_eq!(deserialized, expected);
  }

  #[test]
  fn test_boxed_enum_default() {
    let default_value = BoxedEnum::default();
    let expected = BoxedEnum::Fallback(Box::default());

    assert_eq!(default_value, expected);
  }

  #[test]
  fn test_boxed_partial_eq() {
    let a1 = BoxedEnum::VariantA(Box::new(BoxedTypeA {
      discrim: "boxed_a".to_string(),
      data: "test".to_string(),
    }));
    let a2 = BoxedEnum::VariantA(Box::new(BoxedTypeA {
      discrim: "boxed_a".to_string(),
      data: "test".to_string(),
    }));
    let b = BoxedEnum::VariantB(Box::new(BoxedTypeB {
      discrim: "boxed_b".to_string(),
      count: 42,
    }));

    assert_eq!(a1, a2);
    assert_ne!(a1, b);
  }

  #[test]
  fn test_boxed_clone() {
    let original = BoxedEnum::VariantA(Box::new(BoxedTypeA {
      discrim: "boxed_a".to_string(),
      data: "test".to_string(),
    }));
    let cloned = original.clone();

    assert_eq!(original, cloned);
  }

  // Test for cyclic types with Box
  #[serde_with::skip_serializing_none]
  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct NodeA {
    #[default("node_a".to_string())]
    node_type: String,
    value: String,
    child: Option<Box<CyclicNode>>,
  }

  #[serde_with::skip_serializing_none]
  #[derive(super::Default, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  #[serde(default)]
  struct NodeB {
    #[default("node_b".to_string())]
    node_type: String,
    count: i32,
    child: Option<Box<CyclicNode>>,
  }

  discriminated_enum! {
    enum CyclicNode {
      discriminator: "node_type",
      variants: [
        ("node_a", A(Box<NodeA>)),
        ("node_b", B(Box<NodeB>)),
      ],
    }
  }

  #[test]
  fn test_cyclic_boxed_types() {
    let json = r#"{
      "node_type": "node_a",
      "value": "root",
      "child": {
        "node_type": "node_b",
        "count": 42
      }
    }"#;

    let deserialized: CyclicNode = serde_json::from_str(json).unwrap();

    match deserialized {
      CyclicNode::A(boxed_a) => {
        assert_eq!(boxed_a.value, "root");
        assert!(boxed_a.child.is_some());
        if let Some(child) = boxed_a.child {
          match *child {
            CyclicNode::B(boxed_b) => {
              assert_eq!(boxed_b.count, 42);
            }
            CyclicNode::A(_) => panic!("Expected NodeB variant"),
          }
        }
      }
      CyclicNode::B(_) => panic!("Expected NodeA variant"),
    }
  }

  #[test]
  fn test_cyclic_partial_eq() {
    let node1 = CyclicNode::A(Box::new(NodeA {
      node_type: "node_a".to_string(),
      value: "test".to_string(),
      child: Some(Box::new(CyclicNode::B(Box::new(NodeB {
        node_type: "node_b".to_string(),
        count: 1,
        child: None,
      })))),
    }));

    let node2 = CyclicNode::A(Box::new(NodeA {
      node_type: "node_a".to_string(),
      value: "test".to_string(),
      child: Some(Box::new(CyclicNode::B(Box::new(NodeB {
        node_type: "node_b".to_string(),
        count: 1,
        child: None,
      })))),
    }));

    assert_eq!(node1, node2);
  }

  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  enum QueryParamTestEnum {
    #[serde(rename = "value_1")]
    Value1,
    #[serde(rename = "value_2")]
    Value2,
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "1")]
    One,
  }

  #[test]
  fn test_serialize_query_param_string_enum() {
    let value = QueryParamTestEnum::Value1;
    let result = super::serialize_query_param(&value).unwrap();
    assert_eq!(result, "value_1");

    let value = QueryParamTestEnum::Zero;
    let result = super::serialize_query_param(&value).unwrap();
    assert_eq!(result, "0");
  }

  #[test]
  #[allow(clippy::approx_constant)]
  fn test_serialize_query_param_primitive_types() {
    assert_eq!(super::serialize_query_param(&42).unwrap(), "42");
    assert_eq!(super::serialize_query_param(&true).unwrap(), "true");
    assert_eq!(super::serialize_query_param(&false).unwrap(), "false");
    assert_eq!(super::serialize_query_param(&"hello").unwrap(), "hello");
    assert_eq!(super::serialize_query_param(&3.14).unwrap(), "3.14");
  }

  #[test]
  fn test_serialize_query_param_option_types() {
    let some_value: Option<i32> = Some(42);
    let none_value: Option<i32> = None;
    assert_eq!(super::serialize_query_param(&some_value).unwrap(), "42");
    assert_eq!(super::serialize_query_param(&none_value).unwrap(), "");
  }

  #[test]
  fn test_percent_encode_query_component() {
    assert_eq!(super::percent_encode_query_component("hello world"), "hello%20world");
    assert_eq!(super::percent_encode_query_component("a+b"), "a%2Bb");
    assert_eq!(
      super::percent_encode_query_component("test@example.com"),
      "test%40example.com"
    );
    assert_eq!(super::percent_encode_query_component("simple"), "simple");
    assert_eq!(
      super::percent_encode_query_component("with-dash_underscore"),
      "with-dash_underscore"
    );
  }

  #[test]
  fn test_serialize_and_encode_query_param() {
    let value = QueryParamTestEnum::Value1;
    let serialized = super::serialize_query_param(&value).unwrap();
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "value_1");

    let value = "hello world";
    let serialized = super::serialize_query_param(&value).unwrap();
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "hello%20world");
  }
}
