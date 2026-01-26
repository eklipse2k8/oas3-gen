use std::collections::{BTreeMap, VecDeque};

use petgraph::{Graph, graph::NodeIndex};

use crate::generator::{
  ast::{
    DerivesProvider, DiscriminatedEnumDef, EnumDef, EnumToken, OuterAttr, RustPrimitive, RustType, SerdeImpl,
    SerdeMode, StructDef, StructKind, TypeRef,
  },
  converter::GenerationTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeUsage {
  RequestOnly,
  ResponseOnly,
  Bidirectional,
}

impl TypeUsage {
  pub(super) fn from_flags(in_request: bool, in_response: bool) -> Self {
    match (in_request, in_response) {
      (true, false) => Self::RequestOnly,
      (false, true) => Self::ResponseOnly,
      _ => Self::Bidirectional,
    }
  }

  fn to_serde_mode(self, target: GenerationTarget) -> SerdeMode {
    match (target, self) {
      (GenerationTarget::Client, Self::RequestOnly) | (GenerationTarget::Server, Self::ResponseOnly) => {
        SerdeMode::SerializeOnly
      }
      (GenerationTarget::Client, Self::ResponseOnly) | (GenerationTarget::Server, Self::RequestOnly) => {
        SerdeMode::DeserializeOnly
      }
      (_, Self::Bidirectional) => SerdeMode::Both,
    }
  }
}

type UsageFlags = (bool, bool);

pub(crate) struct SerdeUsage {
  graph: Graph<EnumToken, ()>,
  indices: BTreeMap<EnumToken, NodeIndex>,
  pub(super) usage: BTreeMap<EnumToken, UsageFlags>,
  target: GenerationTarget,
}

impl SerdeUsage {
  pub(crate) fn new(types: &[RustType], seed_usage: BTreeMap<EnumToken, UsageFlags>, target: GenerationTarget) -> Self {
    let (graph, indices) = Self::build_graph(types);
    Self {
      graph,
      indices,
      usage: seed_usage,
      target,
    }
  }

  pub(crate) fn apply(mut self, types: &mut [RustType]) {
    self.propagate();
    self.update_types(types);
  }

  pub(super) fn propagate(&mut self) {
    self.propagate_from_seeds();
    self.propagate_from_orphans();
  }

  fn build_graph(types: &[RustType]) -> (Graph<EnumToken, ()>, BTreeMap<EnumToken, NodeIndex>) {
    let mut graph = Graph::new();
    let mut indices = BTreeMap::new();

    for rust_type in types {
      let type_name: EnumToken = rust_type.type_name().into();
      let idx = *indices
        .entry(type_name.clone())
        .or_insert_with(|| graph.add_node(type_name));

      for dep in Self::dependencies(rust_type) {
        let dep_idx = *indices.entry(dep.clone()).or_insert_with(|| graph.add_node(dep));
        graph.add_edge(idx, dep_idx, ());
      }
    }

    (graph, indices)
  }

  fn dependencies(rust_type: &RustType) -> impl Iterator<Item = EnumToken> + '_ {
    let refs: Box<dyn Iterator<Item = &TypeRef> + '_> = match rust_type {
      RustType::Struct(def) => Box::new(def.fields.iter().map(|f| &f.rust_type)),
      RustType::Enum(def) => Box::new(def.variants.iter().filter_map(|v| v.content.tuple_types()).flatten()),
      RustType::TypeAlias(def) => Box::new(std::iter::once(&def.target)),
      RustType::DiscriminatedEnum(def) => Box::new(
        def
          .variants
          .iter()
          .map(|v| &v.type_name)
          .chain(def.fallback.as_ref().map(|f| &f.type_name)),
      ),
      RustType::ResponseEnum(def) => Box::new(def.variants.iter().filter_map(|v| v.schema_type.as_ref())),
    };

    refs.filter_map(Self::custom_type_name)
  }

  fn custom_type_name(type_ref: &TypeRef) -> Option<EnumToken> {
    match &type_ref.base_type {
      RustPrimitive::Custom(name) => Some(name.as_ref().into()),
      _ => None,
    }
  }

  fn propagate_from_seeds(&mut self) {
    let initial_worklist = self
      .usage
      .iter()
      .filter(|(name, _)| self.indices.contains_key(*name))
      .map(|(name, &flags)| (name.clone(), flags))
      .collect::<VecDeque<_>>();

    self.drain_worklist(initial_worklist);
  }

  fn propagate_from_orphans(&mut self) {
    let orphans = self
      .indices
      .keys()
      .filter(|n| !self.usage.contains_key(*n))
      .cloned()
      .collect::<Vec<_>>();

    let orphan_worklist = orphans
      .into_iter()
      .map(|n| {
        self.usage.insert(n.clone(), (true, true));
        (n, (true, true))
      })
      .collect::<VecDeque<_>>();

    self.drain_worklist(orphan_worklist);
  }

  fn drain_worklist(&mut self, mut worklist: VecDeque<(EnumToken, UsageFlags)>) {
    while let Some((type_name, (in_request, in_response))) = worklist.pop_front() {
      let Some(&idx) = self.indices.get(&type_name) else {
        continue;
      };

      let neighbors = self
        .graph
        .neighbors(idx)
        .filter_map(|dep_idx| self.graph.node_weight(dep_idx).cloned())
        .collect::<Vec<_>>();

      for dep in neighbors {
        let entry = self.usage.entry(dep.clone()).or_insert((false, false));
        let previous = *entry;

        entry.0 |= in_request;
        entry.1 |= in_response;

        if *entry != previous {
          worklist.push_back((dep, *entry));
        }
      }
    }
  }

  fn get_usage(&self, name: &EnumToken) -> TypeUsage {
    self.usage.get(name).map_or(TypeUsage::Bidirectional, |&(req, resp)| {
      TypeUsage::from_flags(req, resp)
    })
  }

  fn update_types(&self, types: &mut [RustType]) {
    for rust_type in types {
      match rust_type {
        RustType::Struct(def) => self.update_struct(def),
        RustType::Enum(def) => self.update_enum(def),
        RustType::DiscriminatedEnum(def) => self.update_discriminated_enum(def),
        RustType::TypeAlias(_) | RustType::ResponseEnum(_) => {}
      }
    }
  }

  fn update_struct(&self, def: &mut StructDef) {
    def.serde_mode = self.struct_serde_mode(def);

    let key: EnumToken = def.name.as_str().into();
    let should_clear_validation = def.kind == StructKind::Schema && self.get_usage(&key) == TypeUsage::ResponseOnly;

    if should_clear_validation {
      def.fields.iter_mut().for_each(|f| f.validation_attrs.clear());
    }

    Self::update_skip_serializing_none(def);
  }

  fn struct_serde_mode(&self, def: &StructDef) -> SerdeMode {
    match def.kind {
      StructKind::Schema => {
        let key: EnumToken = def.name.as_str().into();
        self.get_usage(&key).to_serde_mode(self.target)
      }
      StructKind::OperationRequest | StructKind::HeaderParams => SerdeMode::None,
      StructKind::PathParams => match self.target {
        GenerationTarget::Server => SerdeMode::DeserializeOnly,
        GenerationTarget::Client => SerdeMode::None,
      },
      StructKind::QueryParams => match self.target {
        GenerationTarget::Server => SerdeMode::DeserializeOnly,
        GenerationTarget::Client => SerdeMode::SerializeOnly,
      },
    }
  }

  fn update_skip_serializing_none(def: &mut StructDef) {
    def.outer_attrs.retain(|attr| *attr != OuterAttr::SkipSerializingNone);

    let derives_serialize = def.is_serializable() == SerdeImpl::Derive;
    let has_nullable = def.fields.iter().any(|f| f.rust_type.nullable);
    let is_schema_struct = def.kind != StructKind::OperationRequest;

    if derives_serialize && has_nullable && is_schema_struct {
      def.outer_attrs.push(OuterAttr::SkipSerializingNone);
    }
  }

  fn update_enum(&self, def: &mut EnumDef) {
    def.serde_mode = self.get_usage(&def.name).to_serde_mode(self.target);
  }

  fn update_discriminated_enum(&self, def: &mut DiscriminatedEnumDef) {
    def.serde_mode = self.get_usage(&def.name).to_serde_mode(self.target);
  }
}
