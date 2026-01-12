mod dependency_graph;

use std::collections::{BTreeMap, BTreeSet, VecDeque, btree_map::Entry};

use self::dependency_graph::DependencyGraph;
use crate::generator::{
  ast::{
    ContentCategory, DefaultAtom, DerivesProvider, EnumToken, MethodKind, OperationInfo, OuterAttr, RustPrimitive,
    RustType, SerdeImpl, SerdeMode, StatusCodeToken, StructKind, TypeRef, ValidationAttribute,
    constants::HttpHeaderRef,
  },
  converter::GenerationTarget,
};

pub struct AnalysisOutput {
  pub types: Vec<RustType>,
  pub operations: Vec<OperationInfo>,
  pub header_refs: Vec<HttpHeaderRef>,
}

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

pub(crate) struct TypeAnalyzer {
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
  usage_map: BTreeMap<EnumToken, TypeUsage>,
  target: GenerationTarget,
}

impl TypeAnalyzer {
  pub(crate) fn new(
    types: Vec<RustType>,
    operations: Vec<OperationInfo>,
    seed_usage: BTreeMap<EnumToken, (bool, bool)>,
    target: GenerationTarget,
  ) -> Self {
    let dependency_graph = DependencyGraph::build(&types);
    let usage_map = Self::build_usage_map(seed_usage, &types, &dependency_graph);

    Self {
      types,
      operations,
      usage_map,
      target,
    }
  }

  pub(crate) fn analyze(mut self) -> AnalysisOutput {
    self.deduplicate_response_enums();
    self.add_nested_validation_attrs();
    self.update_serde_modes();

    let header_refs = self.extract_header_refs();
    let types = self.sort_and_dedup_types();

    AnalysisOutput {
      types,
      operations: self.operations,
      header_refs,
    }
  }

  fn sort_and_dedup_types(&self) -> Vec<RustType> {
    let mut map = BTreeMap::new();

    for t in &self.types {
      let name = t.type_name().to_string();
      let priority = t.type_priority();

      match map.entry(name) {
        Entry::Vacant(e) => {
          e.insert(t.clone());
        }
        Entry::Occupied(mut e) => {
          if priority > e.get().type_priority() {
            e.insert(t.clone());
          }
        }
      }
    }

    map.into_values().collect()
  }

  fn extract_header_refs(&self) -> Vec<HttpHeaderRef> {
    self
      .types
      .iter()
      .filter_map(|t| match t {
        RustType::Struct(def) if def.kind == StructKind::HeaderParams => Some(def),
        _ => None,
      })
      .flat_map(|def| &def.fields)
      .filter_map(|field| field.original_name.as_deref())
      .collect::<BTreeSet<_>>()
      .into_iter()
      .map(HttpHeaderRef::from)
      .collect()
  }

  fn build_usage_map(
    mut raw_usage: BTreeMap<EnumToken, (bool, bool)>,
    types: &[RustType],
    dep_graph: &DependencyGraph,
  ) -> BTreeMap<EnumToken, TypeUsage> {
    Self::propagate_usage(&mut raw_usage, dep_graph, types);

    raw_usage
      .into_iter()
      .map(|(name, (req, resp))| (name, TypeUsage::from_flags(req, resp)))
      .collect()
  }

  fn propagate_usage(
    usage_map: &mut BTreeMap<EnumToken, (bool, bool)>,
    dep_graph: &DependencyGraph,
    types: &[RustType],
  ) {
    let mut worklist: VecDeque<(EnumToken, bool, bool)> = usage_map
      .iter()
      .map(|(name, &(req, resp))| (name.clone(), req, resp))
      .collect();

    while let Some((type_name, in_request, in_response)) = worklist.pop_front() {
      if let Some(deps) = dep_graph.dependencies_of(&type_name.to_string()) {
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

    for rust_type in types {
      let type_name: EnumToken = rust_type.type_name().into();
      if !usage_map.contains_key(&type_name) {
        usage_map.insert(type_name.clone(), (true, true));
        worklist.push_back((type_name, true, true));
      }
    }

    while let Some((type_name, in_request, in_response)) = worklist.pop_front() {
      if let Some(deps) = dep_graph.dependencies_of(&type_name.to_string()) {
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
  }

  fn deduplicate_response_enums(&mut self) {
    // Signature includes all media type schemas to properly distinguish response enums
    // that have different inner types (e.g., EventStream<A> vs EventStream<B>)
    type Signature = Vec<(StatusCodeToken, String, Vec<(ContentCategory, String)>)>;

    struct Candidate {
      index: usize,
      name: String,
    }

    let mut signature_map: BTreeMap<Signature, Vec<Candidate>> = BTreeMap::new();

    for (i, rt) in self.types.iter().enumerate() {
      let RustType::ResponseEnum(def) = rt else {
        continue;
      };

      let mut signature: Vec<_> = def
        .variants
        .iter()
        .map(|v| {
          // Include all media types with their schemas in the signature
          let mut media_type_sigs: Vec<_> = v
            .media_types
            .iter()
            .map(|m| {
              let schema_repr = m
                .schema_type
                .as_ref()
                .map_or_else(|| "None".to_string(), TypeRef::to_rust_type);
              (m.category, schema_repr)
            })
            .collect();
          media_type_sigs.sort();
          (v.status_code, v.variant_name.to_string(), media_type_sigs)
        })
        .collect();
      signature.sort();

      signature_map.entry(signature).or_default().push(Candidate {
        index: i,
        name: def.name.to_string(),
      });
    }

    let mut replacements: BTreeMap<String, String> = BTreeMap::new();
    let mut indices_to_remove = BTreeSet::new();

    for group in signature_map.values() {
      if group.len() <= 1 {
        continue;
      }

      let canonical = group
        .iter()
        .min_by(|a, b| a.name.len().cmp(&b.name.len()).then(a.name.cmp(&b.name)))
        .unwrap();

      for candidate in group {
        if candidate.name != canonical.name {
          replacements.insert(candidate.name.clone(), canonical.name.clone());
          indices_to_remove.insert(candidate.index);
        }
      }
    }

    if replacements.is_empty() {
      return;
    }

    for &idx in indices_to_remove.iter().rev() {
      self.types.remove(idx);
    }

    for op in &mut self.operations {
      if let Some(ref current) = op.response_enum
        && let Some(new_name) = replacements.get(&current.to_string())
      {
        op.response_enum = Some(EnumToken::new(new_name));
      }
    }

    for rt in &mut self.types {
      let RustType::Struct(def) = rt else {
        continue;
      };
      for method in &mut def.methods {
        if let MethodKind::ParseResponse { response_enum, .. } = &mut method.kind
          && let Some(new_name) = replacements.get(&response_enum.to_string())
        {
          *response_enum = EnumToken::new(new_name);
        }
      }
    }
  }

  fn add_nested_validation_attrs(&mut self) {
    let mut validated_structs: BTreeSet<DefaultAtom> = self
      .types
      .iter()
      .filter_map(|rt| match rt {
        RustType::Struct(def) if def.has_validation_attrs() => Some(def.name.to_atom()),
        _ => None,
      })
      .collect();

    let mut changed = true;
    while changed {
      changed = false;

      for rust_type in &mut self.types {
        let RustType::Struct(def) = rust_type else {
          continue;
        };

        let mut updated_struct = false;
        for field in &mut def.fields {
          let Some(referenced) = Self::referenced_custom_atom(&field.rust_type) else {
            continue;
          };

          if !validated_structs.contains(&referenced) {
            continue;
          }

          if field.validation_attrs.contains(&ValidationAttribute::Nested) {
            continue;
          }

          field.validation_attrs.push(ValidationAttribute::Nested);
          updated_struct = true;
        }

        if updated_struct && validated_structs.insert(def.name.to_atom()) {
          changed = true;
        }
      }
    }
  }

  fn referenced_custom_atom(type_ref: &TypeRef) -> Option<DefaultAtom> {
    match &type_ref.base_type {
      RustPrimitive::Custom(atom) => Some(atom.clone()),
      _ => None,
    }
  }

  fn update_serde_modes(&mut self) {
    for rust_type in &mut self.types {
      match rust_type {
        RustType::Struct(def) => {
          let key: EnumToken = def.name.as_str().into();
          let usage = self.usage_map.get(&key).copied().unwrap_or(TypeUsage::Bidirectional);

          def.serde_mode = usage.to_serde_mode(self.target);

          if usage == TypeUsage::ResponseOnly {
            for field in &mut def.fields {
              field.validation_attrs.clear();
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
          let usage = self
            .usage_map
            .get(&def.name)
            .copied()
            .unwrap_or(TypeUsage::Bidirectional);
          def.serde_mode = usage.to_serde_mode(self.target);
        }
        RustType::DiscriminatedEnum(def) => {
          let usage = self
            .usage_map
            .get(&def.name)
            .copied()
            .unwrap_or(TypeUsage::Bidirectional);
          def.serde_mode = usage.to_serde_mode(self.target);
        }
        _ => {}
      }
    }
  }
}

#[cfg(test)]
mod tests;
