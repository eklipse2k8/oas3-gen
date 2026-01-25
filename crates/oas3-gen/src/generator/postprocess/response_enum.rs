use std::collections::{BTreeMap, BTreeSet};

use crate::generator::ast::{
  ContentCategory, EnumToken, MethodKind, OperationInfo, ResponseEnumDef, RustType, StatusCodeToken, TypeRef,
};

type Signature = Vec<(StatusCodeToken, String, Vec<(ContentCategory, String)>)>;

struct Candidate {
  index: usize,
  name: String,
}

pub(crate) struct ResponseEnumDeduplicator {
  types: Vec<RustType>,
  operations: Vec<OperationInfo>,
}

impl ResponseEnumDeduplicator {
  pub(crate) fn new(types: Vec<RustType>, operations: Vec<OperationInfo>) -> Self {
    Self { types, operations }
  }

  pub(crate) fn process(mut self) -> (Vec<RustType>, Vec<OperationInfo>) {
    let replacements = self.compute_replacements();

    if replacements.is_empty() {
      return (self.types, self.operations);
    }

    self.apply_replacements(&replacements);
    (self.types, self.operations)
  }

  fn compute_replacements(&mut self) -> BTreeMap<String, String> {
    let signature_map = self.build_signature_map();
    let mut replacements = BTreeMap::new();
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

    for &idx in indices_to_remove.iter().rev() {
      self.types.remove(idx);
    }

    replacements
  }

  fn build_signature_map(&self) -> BTreeMap<Signature, Vec<Candidate>> {
    let mut signature_map = BTreeMap::<Signature, Vec<Candidate>>::new();

    for (i, rt) in self.types.iter().enumerate() {
      let RustType::ResponseEnum(def) = rt else {
        continue;
      };

      let signature = self.compute_signature(def);
      signature_map.entry(signature).or_default().push(Candidate {
        index: i,
        name: def.name.to_string(),
      });
    }

    signature_map
  }

  fn compute_signature(&self, def: &ResponseEnumDef) -> Signature {
    let mut signature: Vec<_> = def
      .variants
      .iter()
      .map(|v| {
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
          .collect::<Vec<_>>();
        media_type_sigs.sort();
        (v.status_code, v.variant_name.to_string(), media_type_sigs)
      })
      .collect::<Vec<_>>();
    signature.sort();
    signature
  }

  fn apply_replacements(&mut self, replacements: &BTreeMap<String, String>) {
    self.update_operations(replacements);
    self.update_struct_methods(replacements);
  }

  fn update_operations(&mut self, replacements: &BTreeMap<String, String>) {
    for op in &mut self.operations {
      if let Some(ref current) = op.response_enum
        && let Some(new_name) = replacements.get(&current.to_string())
      {
        op.response_enum = Some(EnumToken::new(new_name));
      }
    }
  }

  fn update_struct_methods(&mut self, replacements: &BTreeMap<String, String>) {
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
}
