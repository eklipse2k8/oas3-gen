mod dependency_graph;

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use self::dependency_graph::DependencyGraph;

use crate::generator::ast::{
  ContentCategory, DefaultAtom, DerivesProvider, EnumToken, OperationInfo, OuterAttr, RustPrimitive, RustType, SerdeImpl,
  SerdeMode, StatusCodeToken, StructKind, StructMethodKind, TypeRef, ValidationAttribute,
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

  fn to_serde_mode(self) -> SerdeMode {
    match self {
      Self::RequestOnly => SerdeMode::SerializeOnly,
      Self::ResponseOnly => SerdeMode::DeserializeOnly,
      Self::Bidirectional => SerdeMode::Both,
    }
  }
}

pub(crate) struct AnalysisResult {
  pub error_schemas: HashSet<EnumToken>,
}

pub(crate) struct TypeAnalyzer<'a> {
  types: &'a mut Vec<RustType>,
  operations: &'a mut [OperationInfo],
  dependency_graph: DependencyGraph,
  usage_map: BTreeMap<EnumToken, TypeUsage>,
}

impl<'a> TypeAnalyzer<'a> {
  pub(crate) fn new(
    types: &'a mut Vec<RustType>,
    operations: &'a mut [OperationInfo],
    seed_usage: BTreeMap<EnumToken, (bool, bool)>,
  ) -> Self {
    let dependency_graph = DependencyGraph::build(types);
    let usage_map = Self::build_usage_map(seed_usage, types, &dependency_graph);

    Self {
      types,
      operations,
      dependency_graph,
      usage_map,
    }
  }

  pub(crate) fn analyze(mut self) -> AnalysisResult {
    self.deduplicate_response_enums();
    self.add_nested_validation_attrs();
    self.update_serde_modes();

    AnalysisResult {
      error_schemas: self.compute_error_schemas(),
    }
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
    type Signature = Vec<(StatusCodeToken, String, String, ContentCategory)>;

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
          (
            v.status_code,
            v.variant_name.to_string(),
            v.schema_type
              .as_ref()
              .map_or_else(|| "None".to_string(), TypeRef::to_rust_type),
            v.content_category,
          )
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

    for op in self.operations.iter_mut() {
      if let Some(ref current) = op.response_enum
        && let Some(new_name) = replacements.get(&current.to_string())
      {
        op.response_enum = Some(EnumToken::new(new_name));
      }
    }

    for rt in self.types.iter_mut() {
      let RustType::Struct(def) = rt else {
        continue;
      };
      for method in &mut def.methods {
        let StructMethodKind::ParseResponse { response_enum, .. } = &mut method.kind;
        if let Some(new_name) = replacements.get(&response_enum.to_string()) {
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

      for rust_type in self.types.iter_mut() {
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
    for rust_type in self.types.iter_mut() {
      match rust_type {
        RustType::Struct(def) => {
          let key: EnumToken = def.name.as_str().into();
          let usage = self.usage_map.get(&key).copied().unwrap_or(TypeUsage::Bidirectional);

          def.serde_mode = usage.to_serde_mode();

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
          let usage = self.usage_map.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
          def.serde_mode = usage.to_serde_mode();
        }
        RustType::DiscriminatedEnum(def) => {
          let usage = self.usage_map.get(&def.name).copied().unwrap_or(TypeUsage::Bidirectional);
          def.serde_mode = usage.to_serde_mode();
        }
        _ => {}
      }
    }
  }

  fn compute_error_schemas(&self) -> HashSet<EnumToken> {
    let mut error_schemas: HashSet<String> = HashSet::new();
    let mut success_schemas: HashSet<String> = HashSet::new();

    for op in self.operations.iter() {
      error_schemas.extend(op.error_response_types.iter().cloned());
      success_schemas.extend(op.success_response_types.iter().cloned());
    }

    let root_errors: HashSet<String> = error_schemas
      .into_iter()
      .filter(|schema| !success_schemas.contains(schema))
      .collect();

    self.expand_error_types(&root_errors, &success_schemas)
  }

  fn expand_error_types(
    &self,
    roots: &HashSet<String>,
    success_schemas: &HashSet<String>,
  ) -> HashSet<EnumToken> {
    let mut result: HashSet<EnumToken> = roots.iter().map(EnumToken::new).collect();
    let mut queue: Vec<String> = roots.iter().cloned().collect();
    let mut visited: HashSet<String> = HashSet::new();

    while let Some(type_name) = queue.pop() {
      if !visited.insert(type_name.clone()) {
        continue;
      }

      if let Some(deps) = self.dependency_graph.dependencies_of(&type_name) {
        for nested_type in deps {
          if !success_schemas.contains(nested_type) && result.insert(EnumToken::new(nested_type)) {
            queue.push(nested_type.clone());
          }
        }
      }
    }

    result
  }
}

#[cfg(test)]
mod tests;
