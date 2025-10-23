pub use better_default::Default;

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
}
