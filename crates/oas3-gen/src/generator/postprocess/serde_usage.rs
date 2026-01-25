use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::generator::{
  ast::{
    DerivesProvider, EnumToken, OuterAttr, RustPrimitive, RustType, SerdeImpl, SerdeMode, StructKind, TypeRef,
    VariantContent,
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
  fn from_flags(in_request: bool, in_response: bool) -> Self {
    match (in_request, in_response) {
      (true, false) => Self::RequestOnly,
      (false, true) => Self::ResponseOnly,
      (true, true) | (false, false) => Self::Bidirectional,
    }
  }

  fn to_serde_mode(self, target: GenerationTarget) -> SerdeMode {
    match target {
      GenerationTarget::Client => match self {
        Self::RequestOnly => SerdeMode::SerializeOnly,
        Self::ResponseOnly => SerdeMode::DeserializeOnly,
        Self::Bidirectional => SerdeMode::Both,
      },
      GenerationTarget::Server => match self {
        Self::RequestOnly => SerdeMode::DeserializeOnly,
        Self::ResponseOnly => SerdeMode::SerializeOnly,
        Self::Bidirectional => SerdeMode::Both,
      },
    }
  }
}

pub(crate) struct DependencyGraph {
  edges: BTreeMap<String, BTreeSet<String>>,
}

impl DependencyGraph {
  pub(crate) fn build(types: &[RustType]) -> Self {
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();

    for rust_type in types {
      let type_name = rust_type.type_name().to_string();
      let deps = Self::extract_dependencies(rust_type);
      edges.entry(type_name).or_default().extend(deps);
    }

    Self { edges }
  }

  pub(crate) fn dependencies_of(&self, type_name: &str) -> Option<&BTreeSet<String>> {
    self.edges.get(type_name)
  }

  fn extract_dependencies(rust_type: &RustType) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();

    match rust_type {
      RustType::Struct(def) => {
        for field in &def.fields {
          Self::collect_custom_type(&field.rust_type, &mut deps);
        }
      }
      RustType::Enum(def) => {
        for variant in &def.variants {
          if let VariantContent::Tuple(types) = &variant.content {
            for type_ref in types {
              Self::collect_custom_type(type_ref, &mut deps);
            }
          }
        }
      }
      RustType::TypeAlias(def) => {
        Self::collect_custom_type(&def.target, &mut deps);
      }
      RustType::DiscriminatedEnum(def) => {
        for variant in &def.variants {
          Self::collect_custom_type(&variant.type_name, &mut deps);
        }
        if let Some(fallback) = &def.fallback {
          Self::collect_custom_type(&fallback.type_name, &mut deps);
        }
      }
      RustType::ResponseEnum(def) => {
        for variant in &def.variants {
          if let Some(type_ref) = &variant.schema_type {
            Self::collect_custom_type(type_ref, &mut deps);
          }
        }
      }
    }

    deps
  }

  fn collect_custom_type(type_ref: &TypeRef, deps: &mut BTreeSet<String>) {
    if let RustPrimitive::Custom(name) = &type_ref.base_type {
      deps.insert(name.to_string());
    }
  }
}

pub(crate) struct UsagePropagator {
  usage_map: BTreeMap<EnumToken, TypeUsage>,
  target: GenerationTarget,
}

impl UsagePropagator {
  pub(crate) fn new(
    types: &[RustType],
    seed_usage: BTreeMap<EnumToken, (bool, bool)>,
    target: GenerationTarget,
  ) -> Self {
    let dependency_graph = DependencyGraph::build(types);
    let usage_map = Self::build_usage_map(seed_usage, types, &dependency_graph);

    Self { usage_map, target }
  }

  pub(crate) fn propagate(&mut self, types: &mut [RustType]) {
    self.update_serde_modes(types);
  }

  pub(crate) fn build_usage_map(
    mut raw_usage: BTreeMap<EnumToken, (bool, bool)>,
    types: &[RustType],
    dep_graph: &DependencyGraph,
  ) -> BTreeMap<EnumToken, TypeUsage> {
    Self::propagate_usage(&mut raw_usage, dep_graph, types);

    raw_usage
      .into_iter()
      .map(|(name, (req, resp))| (name, TypeUsage::from_flags(req, resp)))
      .collect::<BTreeMap<_, _>>()
  }

  fn propagate_usage(
    usage_map: &mut BTreeMap<EnumToken, (bool, bool)>,
    dep_graph: &DependencyGraph,
    types: &[RustType],
  ) {
    let mut worklist: VecDeque<(EnumToken, bool, bool)> = usage_map
      .iter()
      .map(|(name, &(req, resp))| (name.clone(), req, resp))
      .collect::<VecDeque<_>>();

    Self::drain_worklist(usage_map, dep_graph, &mut worklist);

    for rust_type in types {
      let type_name: EnumToken = rust_type.type_name().into();
      if !usage_map.contains_key(&type_name) {
        usage_map.insert(type_name.clone(), (true, true));
        worklist.push_back((type_name, true, true));
      }
    }

    Self::drain_worklist(usage_map, dep_graph, &mut worklist);
  }

  fn drain_worklist(
    usage_map: &mut BTreeMap<EnumToken, (bool, bool)>,
    dep_graph: &DependencyGraph,
    worklist: &mut VecDeque<(EnumToken, bool, bool)>,
  ) {
    while let Some((type_name, in_request, in_response)) = worklist.pop_front() {
      let Some(deps) = dep_graph.dependencies_of(&type_name.to_string()) else {
        continue;
      };

      for dep in deps {
        let dep_token: EnumToken = dep.as_str().into();
        let entry = usage_map.entry(dep_token.clone()).or_insert((false, false));
        let old_value = *entry;

        entry.0 |= in_request;
        entry.1 |= in_response;

        if *entry != old_value {
          worklist.push_back((dep_token, entry.0, entry.1));
        }
      }
    }
  }

  fn update_serde_modes(&mut self, types: &mut [RustType]) {
    let usage_map = &self.usage_map;
    let target = self.target;

    let get_usage = |name: &EnumToken| usage_map.get(name).copied().unwrap_or(TypeUsage::Bidirectional);

    for rust_type in types.iter_mut() {
      match rust_type {
        RustType::Struct(def) => {
          def.serde_mode = Self::struct_serde_mode(def.kind, target, || {
            let key: EnumToken = def.name.as_str().into();
            get_usage(&key).to_serde_mode(target)
          });

          if def.kind == StructKind::Schema {
            let key: EnumToken = def.name.as_str().into();
            if get_usage(&key) == TypeUsage::ResponseOnly {
              for field in &mut def.fields {
                field.validation_attrs.clear();
              }
            }
          }

          def.outer_attrs.retain(|attr| *attr != OuterAttr::SkipSerializingNone);
          let derives_serialize = def.is_serializable() == SerdeImpl::Derive;
          let has_nullable = def.fields.iter().any(|f| f.rust_type.nullable);
          if derives_serialize && has_nullable && def.kind != StructKind::OperationRequest {
            def.outer_attrs.push(OuterAttr::SkipSerializingNone);
          }
        }
        RustType::Enum(def) => {
          def.serde_mode = get_usage(&def.name).to_serde_mode(target);
        }
        RustType::DiscriminatedEnum(def) => {
          def.serde_mode = get_usage(&def.name).to_serde_mode(target);
        }
        _ => {}
      }
    }
  }

  fn struct_serde_mode(
    kind: StructKind,
    target: GenerationTarget,
    schema_mode: impl FnOnce() -> SerdeMode,
  ) -> SerdeMode {
    match kind {
      StructKind::Schema => schema_mode(),
      StructKind::OperationRequest | StructKind::HeaderParams => SerdeMode::None,
      StructKind::PathParams => match target {
        GenerationTarget::Server => SerdeMode::DeserializeOnly,
        GenerationTarget::Client => SerdeMode::None,
      },
      StructKind::QueryParams => match target {
        GenerationTarget::Server => SerdeMode::DeserializeOnly,
        GenerationTarget::Client => SerdeMode::SerializeOnly,
      },
    }
  }
}
