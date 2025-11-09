pub use better_default::Default;
pub use http::Method;
pub use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
pub use serde_with::skip_serializing_none;

#[doc(hidden)]
#[macro_export]
macro_rules! discriminated_enum_default_helper {
  ($fallback_type:ty, $constructor:expr) => {
    $constructor(<$fallback_type>::default())
  };
}

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

pub const PATH_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[inline]
#[must_use]
pub fn percent_encode_path_segment(segment: &str) -> String {
  utf8_percent_encode(segment, PATH_ENCODE_SET).to_string()
}

pub const QUERY_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[inline]
#[must_use]
pub fn percent_encode_query_component(component: &str) -> String {
  utf8_percent_encode(component, QUERY_ENCODE_SET).to_string()
}

#[inline]
pub fn serialize_query_param<T: serde::Serialize>(value: &T) -> String {
  serde_plain::to_string(value).unwrap_or_else(|_| String::new())
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
    let result = super::serialize_query_param(&value);
    assert_eq!(result, "value_1");

    let value = QueryParamTestEnum::Zero;
    let result = super::serialize_query_param(&value);
    assert_eq!(result, "0");
  }

  #[test]
  #[allow(clippy::approx_constant)]
  fn test_serialize_query_param_primitive_types() {
    assert_eq!(super::serialize_query_param(&42), "42");
    assert_eq!(super::serialize_query_param(&true), "true");
    assert_eq!(super::serialize_query_param(&false), "false");
    assert_eq!(super::serialize_query_param(&"hello"), "hello");
    assert_eq!(super::serialize_query_param(&3.14), "3.14");
  }

  #[test]
  fn test_serialize_query_param_option_types() {
    let some_value: Option<i32> = Some(42);
    let none_value: Option<i32> = None;
    assert_eq!(super::serialize_query_param(&some_value), "42");
    assert_eq!(super::serialize_query_param(&none_value), "");
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
    let serialized = super::serialize_query_param(&value);
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "value_1");

    let value = "hello world";
    let serialized = super::serialize_query_param(&value);
    let encoded = super::percent_encode_query_component(&serialized);
    assert_eq!(encoded, "hello%20world");
  }
}
