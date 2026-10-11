use std::collections::{BTreeMap, BTreeSet};

use string_cache::DefaultAtom;

use super::serde_usage::SerdeUsage;
use crate::{
  generator::{
    ast::{
      DiscriminatedEnumDef, EnumToken, FieldDef, FrameData, FrameMember, OperationInfo, RustPrimitive, RustType,
      StreamItem, StructDef, TypeAliasDef, TypeAliasToken, TypeRef,
    },
    converter::GenerationTarget,
    naming::responses::{client_stream_type, sse_frame_type},
  },
  utils::spec::is_frame_type,
};

/// Marks the `text/event-stream` items that are frame types, and the media types that
/// stream them, so the client decodes and the server encodes whole frames.
///
/// A frame type is a struct whose fields all come from `data`, `event`, `id`, and
/// `retry`, `data` among them, or a discriminated union keyed by `event` whose variants
/// are all such structs and that nothing but a stream refers to. Any other item is the
/// payload of each event's `data`.
///
/// A client reads every item as an `oas3_gen_support::SseFrame`. A frame struct only
/// streams refer to becomes an alias of the frame it reads; one that other types or
/// bodies also use stays a struct, and streams read the frame type in its place. A
/// stream of payloads reads each one in a frame.
pub struct FrameTypes;

impl FrameTypes {
  pub fn process(types: &mut [RustType], operations: &mut [OperationInfo], target: GenerationTarget) {
    let items = operations
      .iter()
      .filter_map(|op| op.response_variants.as_deref())
      .flatten()
      .flat_map(|variant| &variant.media_types)
      .filter(|media_type| media_type.stream.is_some())
      .filter_map(|media_type| item_name(media_type.schema_type.as_ref()?))
      .collect::<BTreeSet<_>>();

    let frame_structs = types
      .iter()
      .filter_map(|rust_type| match rust_type {
        RustType::Struct(def) if is_frame_type(def.fields.iter().map(FieldDef::serde_name)) => {
          Some((def.name.to_atom(), FrameStruct::of(def)))
        }
        _ => None,
      })
      .collect::<BTreeMap<_, _>>();
    let mut frame_unions = types
      .iter()
      .filter_map(|rust_type| match rust_type {
        RustType::DiscriminatedEnum(def) if items.contains(&def.name.to_atom()) => {
          Some((def.name.to_atom(), keyed_by_event(def, &frame_structs)?))
        }
        _ => None,
      })
      .collect::<BTreeMap<_, _>>();
    let candidates = frame_struct_names(types, &items, &frame_structs, &frame_unions);
    let referenced = referenced_outside_frames(types, operations, &candidates, &frame_unions);
    frame_unions.retain(|name, _| !referenced.contains(name));
    let frame_names = frame_struct_names(types, &items, &frame_structs, &frame_unions);
    let frames = frame_structs
      .iter()
      .filter(|(name, _)| frame_names.contains(*name))
      .collect::<BTreeMap<_, _>>();
    let client_inline = |name: &DefaultAtom| {
      frames
        .get(name)
        .filter(|_| target == GenerationTarget::Client && referenced.contains(name))
        .map(|frame| frame.client_type.clone())
    };

    for rust_type in types.iter_mut() {
      match rust_type {
        RustType::Struct(def) if frames.contains_key(&def.name.to_atom()) => {
          def.sse_frame = true;
          let name = def.name.to_atom();
          if target == GenerationTarget::Client && !referenced.contains(&name) {
            let alias = TypeAliasDef {
              name: TypeAliasToken::new(&name),
              docs: def.docs.clone(),
              target: TypeRef::new(frames[&name].client_type.as_str()),
            };
            *rust_type = RustType::TypeAlias(alias);
          }
        }
        RustType::DiscriminatedEnum(def) if frame_unions.contains_key(&def.name.to_atom()) => {
          def.sse_frame = true;
          for (variant, member) in def.variants.iter_mut().zip(&frame_unions[&def.name.to_atom()]) {
            variant.frame = Some(*member);
            if let Some(frame_type) = item_name(&variant.type_name).as_ref().and_then(client_inline) {
              variant.type_name.base_type = RustPrimitive::Custom(frame_type.into());
            }
          }
        }
        _ => {}
      }
    }

    for variant in operations
      .iter_mut()
      .filter_map(|op| op.response_variants.as_deref_mut())
      .flatten()
    {
      let mut client_item = None;
      for media_type in &mut variant.media_types {
        let Some(stream) = &mut media_type.stream else {
          continue;
        };
        let item = media_type.schema_type.as_ref().and_then(item_name);
        stream.item = match &item {
          Some(name) if frame_unions.contains_key(name) => StreamItem::FrameUnion,
          Some(name) if frames.contains_key(name) => StreamItem::Frame(frames[name].data),
          _ => StreamItem::Payload,
        };
        client_item = match stream.item {
          StreamItem::Payload => media_type
            .schema_type
            .as_ref()
            .map(|payload| sse_frame_type(&payload.to_rust_type(), None)),
          StreamItem::Frame(_) => item.as_ref().and_then(client_inline),
          StreamItem::FrameUnion => None,
        };
      }
      if target == GenerationTarget::Client
        && let Some(client_item) = client_item
        && variant.schema_type.is_some()
      {
        variant.schema_type = Some(TypeRef::new(client_stream_type(&client_item)));
      }
    }
  }
}

/// What the client and server need to know of a frame struct.
#[derive(Debug)]
struct FrameStruct {
  data: FrameData,
  declares_event: bool,
  /// The frame type a client reads it as: the frame of its `data` and `event` types.
  client_type: String,
}

impl FrameStruct {
  fn of(def: &StructDef) -> Self {
    let field = |name: &str| def.fields.iter().find(|field| field.serde_name() == name);
    let data = field("data").map_or_else(|| TypeRef::new(RustPrimitive::String), |field| field.rust_type.clone());
    let event = field("event");
    let event_type = event
      .filter(|field| !field.rust_type.is_string_like())
      .map(|field| field.rust_type.to_rust_type());
    Self {
      data: FrameData::of(&data),
      declares_event: event.is_some(),
      client_type: sse_frame_type(&data.to_rust_type(), event_type.as_deref()),
    }
  }
}

/// The name of the generated type a stream item refers to, when it is one.
fn item_name(item: &TypeRef) -> Option<DefaultAtom> {
  match &item.base_type {
    RustPrimitive::Custom(name) if !item.is_array && item.map_value.is_none() => Some(name.clone()),
    _ => None,
  }
}

/// How each variant of a union keyed by `event` travels, when every variant is a frame
/// struct.
fn keyed_by_event(
  def: &DiscriminatedEnumDef,
  frame_structs: &BTreeMap<DefaultAtom, FrameStruct>,
) -> Option<Vec<FrameMember>> {
  if def.discriminator_field != "event" || def.fallback.is_some() {
    return None;
  }
  def
    .variants
    .iter()
    .map(|variant| {
      let frame = frame_structs.get(&item_name(&variant.type_name)?)?;
      Some(FrameMember {
        data: frame.data,
        declares_event: frame.declares_event,
      })
    })
    .collect()
}

/// The frame structs streams send: those streamed on their own, and the members of
/// `frame_unions`.
fn frame_struct_names(
  types: &[RustType],
  items: &BTreeSet<DefaultAtom>,
  frame_structs: &BTreeMap<DefaultAtom, FrameStruct>,
  frame_unions: &BTreeMap<DefaultAtom, Vec<FrameMember>>,
) -> BTreeSet<DefaultAtom> {
  let members = types
    .iter()
    .filter_map(|rust_type| match rust_type {
      RustType::DiscriminatedEnum(def) if frame_unions.contains_key(&def.name.to_atom()) => Some(&def.variants),
      _ => None,
    })
    .flatten()
    .filter_map(|variant| item_name(&variant.type_name));
  items
    .iter()
    .filter(|item| frame_structs.contains_key(*item))
    .cloned()
    .chain(members)
    .collect()
}

/// The types that something other than a stream refers to: a type that isn't one of the
/// frame types `frame_structs` and `frame_unions`, or a response body that isn't an event
/// stream.
fn referenced_outside_frames(
  types: &[RustType],
  operations: &[OperationInfo],
  frame_structs: &BTreeSet<DefaultAtom>,
  frame_unions: &BTreeMap<DefaultAtom, Vec<FrameMember>>,
) -> BTreeSet<DefaultAtom> {
  let declared = types
    .iter()
    .map(|rust_type| EnumToken::from(rust_type.type_name()))
    .collect::<BTreeSet<_>>();
  let from_types = types
    .iter()
    .filter(|rust_type| {
      let name = rust_type.type_name();
      !frame_structs.contains(&name) && !frame_unions.contains_key(&name)
    })
    .flat_map(|rust_type| SerdeUsage::dependencies(rust_type, &declared));
  let from_bodies = operations
    .iter()
    .filter_map(|op| op.response_variants.as_deref())
    .flatten()
    .filter(|variant| variant.media_types.iter().all(|media_type| media_type.stream.is_none()))
    .filter_map(|variant| variant.schema_type.as_ref())
    .flat_map(|schema_type| SerdeUsage::custom_type_names(schema_type, &declared));
  from_types.chain(from_bodies).map(|name| name.to_atom()).collect()
}
