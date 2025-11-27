use std::collections::{BTreeMap, VecDeque};

use super::type_graph::TypeDependencyGraph;
use crate::generator::ast::{EnumToken, RustType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeUsage {
  RequestOnly,
  ResponseOnly,
  Bidirectional,
}

pub(crate) fn build_type_usage_map(
  mut usage_map: BTreeMap<EnumToken, (bool, bool)>,
  types: &[RustType],
) -> BTreeMap<EnumToken, TypeUsage> {
  let dep_graph = TypeDependencyGraph::build(types);
  propagate_usage_to_all_dependencies(&mut usage_map, &dep_graph, types);

  usage_map
    .into_iter()
    .map(|(type_name, (in_request, in_response))| {
      let usage = match (in_request, in_response) {
        (true, false) => TypeUsage::RequestOnly,
        (false, true) => TypeUsage::ResponseOnly,
        (true, true) | (false, false) => TypeUsage::Bidirectional,
      };
      (type_name, usage)
    })
    .collect()
}

fn propagate_usage_to_all_dependencies(
  usage_map: &mut BTreeMap<EnumToken, (bool, bool)>,
  dep_graph: &TypeDependencyGraph,
  types: &[RustType],
) {
  let mut worklist = VecDeque::new();

  for (type_name, &(in_request, in_response)) in usage_map.iter() {
    worklist.push_back((type_name.clone(), in_request, in_response));
  }

  while let Some((type_name, in_request, in_response)) = worklist.pop_front() {
    if let Some(deps) = dep_graph.get_dependencies(&type_name.to_string()) {
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
    if let Some(deps) = dep_graph.get_dependencies(&type_name.to_string()) {
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
