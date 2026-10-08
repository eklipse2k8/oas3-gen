use std::collections::{BTreeSet, HashMap, HashSet};

use indexmap::IndexMap;

use super::serde_usage::SerdeUsage;
use crate::generator::{
  ast::{
    ContentCategory, DefaultAtom, EnumToken, FieldDef, MultipartFieldInfo, MultipartPartKind, MultipartProperty,
    OperationBody, OperationInfo, PartMembers, PartType, PartValue, RustPrimitive, RustType, SerdeAsFieldAttr,
    SerdeAttribute, SerdeMode, StructKind, TypeRef, is_json_media_type,
  },
  metrics::GenerationWarning,
  naming::constants::OCTET_STREAM_MEDIA_TYPE,
};

/// Resolves the parts each `multipart/form-data` request body's struct fields become.
///
/// Runs after type deduplication so field types are final. OpenAPI 3.1 reads a multipart
/// property whose schema declares no `type` as raw binary, so such a `serde_json::Value` field is
/// retyped to bytes first, unless its struct is also used outside multipart bodies, where the
/// retype would change its JSON form.
pub(crate) struct MultipartProcessor<'a> {
  types: &'a [RustType],
  positions: HashMap<DefaultAtom, usize>,
}

impl<'a> MultipartProcessor<'a> {
  /// Fills each multipart body's `multipart_fields` and returns warnings for raw binary
  /// properties left untyped because their struct is shared.
  pub(crate) fn process(types: &'a mut [RustType], operations: &mut [OperationInfo]) -> Vec<GenerationWarning> {
    if !operations.iter().any(|operation| multipart_body(operation).is_some()) {
      return vec![];
    }
    let positions = types
      .iter()
      .enumerate()
      .map(|(position, rust_type)| (rust_type.type_name(), position))
      .collect::<HashMap<_, _>>();
    let warnings = retype_raw_binary(types, operations, &positions);

    let processor = Self { types, positions };
    for body in operations.iter_mut().filter_map(|operation| operation.body.as_mut()) {
      if body.content_category != ContentCategory::Multipart {
        continue;
      }
      let Some(RustType::Struct(def)) = body_type_name(body).and_then(|name| processor.lookup(name)) else {
        continue;
      };
      body.multipart_fields = Some(
        def
          .fields
          .iter()
          .filter_map(|field| processor.field_info(field, &body.multipart_properties))
          .collect(),
      );
    }
    warnings
  }

  fn lookup(&self, name: &DefaultAtom) -> Option<&'a RustType> {
    self.positions.get(name).map(|position| &self.types[*position])
  }

  fn field_info(
    &self,
    field: &FieldDef,
    properties: &IndexMap<String, MultipartProperty>,
  ) -> Option<MultipartFieldInfo> {
    if field.serde_attrs.contains(&SerdeAttribute::Skip) {
      return None;
    }

    let property = properties.get(field.serde_name());
    let content_type = property.and_then(|property| property.content_type.clone());
    let serialize_as = match &field.serde_as_attr {
      Some(SerdeAsFieldAttr::CustomOverride { custom_type, .. }) => Some(custom_type.clone()),
      _ => None,
    };

    let kind = if field.serde_attrs.contains(&SerdeAttribute::Flatten) {
      let entry_type = field
        .rust_type
        .map_value
        .as_deref()
        .cloned()
        .unwrap_or_else(|| TypeRef::new(RustPrimitive::Value));
      MultipartPartKind::Flattened {
        entry: self.part_type(&entry_type),
        repeated: entry_type.is_array,
      }
    } else if field.rust_type.base_type == RustPrimitive::Bytes && serialize_as.is_none() {
      MultipartPartKind::File {
        content_type: content_type.unwrap_or_else(|| OCTET_STREAM_MEDIA_TYPE.to_owned()),
      }
    } else {
      let value = match (&serialize_as, &content_type) {
        (_, Some(content_type)) if is_json_media_type(content_type) => PartValue::Json,
        (Some(_), _) if field.rust_type.base_type == RustPrimitive::Bytes => PartValue::SerdeString,
        (Some(_), _) => PartValue::Dynamic,
        (None, _) => self.part_value(&field.rust_type),
      };
      match property.and_then(|property| property.rfc6570) {
        Some(rfc6570) => MultipartPartKind::Styled {
          value,
          rfc6570,
          members: (!field.rust_type.is_array)
            .then(|| self.members(&field.rust_type))
            .flatten(),
        },
        None => MultipartPartKind::Value { value, content_type },
      }
    };

    Some(
      MultipartFieldInfo::builder()
        .name(field.name.clone())
        .part_name(field.serde_name())
        .rust_type(field.rust_type.clone())
        .kind(kind)
        .maybe_serialize_as(serialize_as)
        .deprecated(field.deprecated)
        .build(),
    )
  }

  /// Members of a struct- or map-typed property, for RFC 6570 object serialization.
  fn members(&self, type_ref: &TypeRef) -> Option<PartMembers> {
    if let Some(value_type) = type_ref.map_value.as_deref() {
      return Some(PartMembers::Map(self.member_type(value_type)));
    }
    let RustPrimitive::Custom(name) = &type_ref.base_type else {
      return None;
    };
    let Some(RustType::Struct(def)) = self.lookup(name) else {
      return None;
    };
    let members = def
      .fields
      .iter()
      .filter(|field| !field.serde_attrs.contains(&SerdeAttribute::Skip))
      .filter(|field| !field.serde_attrs.contains(&SerdeAttribute::Flatten))
      .map(|field| (field.serde_name().to_owned(), self.member_type(&field.rust_type)))
      .collect();
    Some(PartMembers::Named(members))
  }

  /// The type of an object member. RFC 6570 defines no form for nested arrays, so an array
  /// member travels as JSON text.
  fn member_type(&self, type_ref: &TypeRef) -> PartType {
    let mut member_type = self.part_type(type_ref);
    if type_ref.is_array {
      member_type.value = PartValue::Json;
    }
    member_type
  }

  /// The type of one value of `type_ref`, without its `Option` or collection wrappers.
  fn part_type(&self, type_ref: &TypeRef) -> PartType {
    let rust_type = type_ref.element_type();
    PartType {
      value: self.part_value(&rust_type),
      rust_type,
    }
  }

  fn part_value(&self, type_ref: &TypeRef) -> PartValue {
    match &type_ref.base_type {
      RustPrimitive::String | RustPrimitive::StaticStr => PartValue::String,
      RustPrimitive::Date | RustPrimitive::DateTime | RustPrimitive::Time | RustPrimitive::Uuid => {
        PartValue::SerdeString
      }
      RustPrimitive::Value | RustPrimitive::Bytes | RustPrimitive::Duration | RustPrimitive::Unit => PartValue::Dynamic,
      RustPrimitive::Custom(name) => match self.lookup(name) {
        Some(RustType::Struct(_) | RustType::DiscriminatedEnum(_)) => PartValue::Json,
        Some(RustType::Enum(def)) if def.generate_display && def.scalar_repr.is_some() => PartValue::Scalar,
        Some(RustType::Enum(def)) if def.generate_display => PartValue::StringEnum,
        Some(RustType::TypeAlias(alias)) if !alias.target.is_array => self.part_value(&alias.target),
        _ => PartValue::Dynamic,
      },
      _ => PartValue::Scalar,
    }
  }
}

/// Retypes the raw binary `serde_json::Value` fields of multipart body structs to bytes, and
/// warns instead for structs also used outside multipart bodies.
fn retype_raw_binary(
  types: &mut [RustType],
  operations: &[OperationInfo],
  positions: &HashMap<DefaultAtom, usize>,
) -> Vec<GenerationWarning> {
  let targets = operations
    .iter()
    .filter_map(|operation| {
      let body = multipart_body(operation)?;
      let position = *positions.get(body_type_name(body)?)?;
      let RustType::Struct(def) = &types[position] else {
        return None;
      };
      let fields = def
        .fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.rust_type.base_type == RustPrimitive::Value)
        .filter(|(_, field)| {
          body
            .multipart_properties
            .get(field.serde_name())
            .is_some_and(|property| property.raw_binary)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
      (!fields.is_empty()).then_some((operation, position, fields))
    })
    .collect::<Vec<_>>();
  if targets.is_empty() {
    return vec![];
  }

  let shared = shared_types(types, operations);
  let mut warnings = vec![];
  for (operation, position, fields) in targets {
    let RustType::Struct(def) = &mut types[position] else {
      continue;
    };
    if shared.contains(&def.name.to_atom()) {
      warnings.extend(fields.iter().map(|&index| {
        GenerationWarning::operation_specific(
          &operation.operation_id,
          &format!(
            "multipart property `{}` is raw binary, but `{}` is also used outside multipart bodies, so it stays \
             `serde_json::Value` and is sent as text",
            def.fields[index].serde_name(),
            def.name
          ),
        )
      }));
    } else {
      for index in fields {
        def.fields[index].rust_type.base_type = RustPrimitive::Bytes;
      }
    }
  }
  warnings
}

fn multipart_body(operation: &OperationInfo) -> Option<&OperationBody> {
  operation
    .body
    .as_ref()
    .filter(|body| body.content_category == ContentCategory::Multipart)
}

fn body_type_name(body: &OperationBody) -> Option<&DefaultAtom> {
  match &body.body_type.as_ref()?.base_type {
    RustPrimitive::Custom(name) => Some(name),
    _ => None,
  }
}

/// Names of types used outside multipart request bodies: as a response (serialized both ways),
/// as another operation's body, or inside another schema type.
fn shared_types(types: &[RustType], operations: &[OperationInfo]) -> HashSet<DefaultAtom> {
  let declared = types
    .iter()
    .map(|rust_type| EnumToken::from(rust_type.type_name()))
    .collect::<BTreeSet<_>>();
  let referenced = types
    .iter()
    .filter(|rust_type| !matches!(rust_type, RustType::Struct(def) if def.kind == StructKind::OperationRequest))
    .flat_map(|rust_type| SerdeUsage::dependencies(rust_type, &declared))
    .map(|name| name.to_atom());
  let bidirectional = types.iter().filter_map(|rust_type| match rust_type {
    RustType::Struct(def) if def.serde_mode == SerdeMode::Both => Some(def.name.to_atom()),
    _ => None,
  });
  let other_bodies = operations
    .iter()
    .filter_map(|operation| operation.body.as_ref())
    .filter(|body| body.content_category != ContentCategory::Multipart)
    .filter_map(body_type_name)
    .cloned();
  referenced.chain(bidirectional).chain(other_bodies).collect()
}
