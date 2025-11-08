use std::collections::BTreeSet;

use proc_macro2::TokenStream;

use super::{TypeUsage, attributes};
use crate::generator::ast::StructKind;

const DEBUG: &str = "Debug";
const CLONE: &str = "Clone";
const SERIALIZE: &str = "Serialize";
const DESERIALIZE: &str = "Deserialize";
const VALIDATE: &str = "validator::Validate";
const DEFAULT: &str = "oas3_gen_support::Default";

type DeriveSet = BTreeSet<String>;

pub(crate) struct DeriveManager {
  derives: DeriveSet,
}

impl DeriveManager {
  fn new(initial_derives: &[String]) -> Self {
    Self {
      derives: initial_derives.iter().cloned().collect(),
    }
  }

  fn add(&mut self, derive: &str) -> &mut Self {
    self.derives.insert(derive.to_string());
    self
  }

  fn apply_schema_derives(&mut self, usage: Option<&TypeUsage>) -> &mut Self {
    if let Some(usage) = usage {
      match usage {
        TypeUsage::RequestOnly => {
          self.add(SERIALIZE).add(VALIDATE);
        }
        TypeUsage::ResponseOnly => {
          self.add(DESERIALIZE);
        }
        TypeUsage::Bidirectional => {
          self.add(SERIALIZE).add(DESERIALIZE).add(VALIDATE);
        }
      }
    }
    self
  }

  fn apply_request_body_derives(&mut self, usage: Option<&TypeUsage>) -> &mut Self {
    self
      .add(DEBUG)
      .add(CLONE)
      .apply_schema_derives(usage.or(Some(&TypeUsage::Bidirectional)))
      .add(DEFAULT)
  }

  fn apply_operation_request_derives(&mut self) -> &mut Self {
    self.add(DEBUG).add(CLONE).add(VALIDATE).add(DEFAULT)
  }

  pub fn for_struct(def_derives: &[String], kind: StructKind, usage: Option<&TypeUsage>) -> Self {
    match kind {
      StructKind::OperationRequest => {
        let mut manager = Self::new(&[]);
        manager.apply_operation_request_derives();
        manager
      }
      StructKind::RequestBody => {
        let mut manager = Self::new(&[]);
        manager.apply_request_body_derives(usage);
        manager
      }
      StructKind::Schema => {
        let mut manager = Self::new(def_derives);
        manager.apply_schema_derives(usage);
        manager
      }
    }
  }

  pub fn for_enum(def_derives: &[String]) -> Self {
    Self::new(def_derives)
  }

  pub fn to_token_stream(&self) -> TokenStream {
    let derives_vec: Vec<_> = self.derives.iter().cloned().collect();
    attributes::generate_derives_from_slice(&derives_vec)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_derive_manager_operation_request() {
    let manager = DeriveManager::for_struct(&[], StructKind::OperationRequest, None);
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("validator :: Validate"));
    assert!(tokens.contains("oas3_gen_support :: Default"));
    assert!(!tokens.contains("Serialize"));
    assert!(!tokens.contains("Deserialize"));
  }

  #[test]
  fn test_derive_manager_request_body_request_only() {
    let manager = DeriveManager::for_struct(&[], StructKind::RequestBody, Some(&TypeUsage::RequestOnly));
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Serialize"));
    assert!(tokens.contains("validator :: Validate"));
    assert!(tokens.contains("oas3_gen_support :: Default"));
    assert!(!tokens.contains("Deserialize"));
  }

  #[test]
  fn test_derive_manager_request_body_response_only() {
    let manager = DeriveManager::for_struct(&[], StructKind::RequestBody, Some(&TypeUsage::ResponseOnly));
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Deserialize"));
    assert!(tokens.contains("oas3_gen_support :: Default"));
    assert!(!tokens.contains("Serialize"));
    assert!(!tokens.contains("validator :: Validate"));
  }

  #[test]
  fn test_derive_manager_request_body_bidirectional() {
    let manager = DeriveManager::for_struct(&[], StructKind::RequestBody, Some(&TypeUsage::Bidirectional));
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Serialize"));
    assert!(tokens.contains("Deserialize"));
    assert!(tokens.contains("validator :: Validate"));
    assert!(tokens.contains("oas3_gen_support :: Default"));
  }

  #[test]
  fn test_derive_manager_schema_request_only() {
    let initial = vec!["Debug".to_string(), "Clone".to_string()];
    let manager = DeriveManager::for_struct(&initial, StructKind::Schema, Some(&TypeUsage::RequestOnly));
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Serialize"));
    assert!(tokens.contains("validator :: Validate"));
    assert!(!tokens.contains("Deserialize"));
  }

  #[test]
  fn test_derive_manager_schema_response_only() {
    let initial = vec!["Debug".to_string(), "Clone".to_string()];
    let manager = DeriveManager::for_struct(&initial, StructKind::Schema, Some(&TypeUsage::ResponseOnly));
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Deserialize"));
    assert!(!tokens.contains("Serialize"));
  }

  #[test]
  fn test_derive_manager_enum() {
    let initial = vec![
      "Debug".to_string(),
      "Clone".to_string(),
      "Serialize".to_string(),
      "Deserialize".to_string(),
    ];
    let manager = DeriveManager::for_enum(&initial);
    let tokens = manager.to_token_stream().to_string();

    assert!(tokens.contains("Debug"));
    assert!(tokens.contains("Clone"));
    assert!(tokens.contains("Serialize"));
    assert!(tokens.contains("Deserialize"));
    assert_eq!(manager.derives.len(), 4);
  }

  #[test]
  fn test_derive_manager_no_duplicates() {
    let initial = vec!["Debug".to_string(), "Clone".to_string()];
    let mut manager = DeriveManager::new(&initial);
    manager.add(DEBUG);
    manager.add(CLONE);
    manager.add(SERIALIZE);

    assert_eq!(
      manager.derives.len(),
      3,
      "Should not add duplicates and should contain the new derive"
    );
    assert!(manager.derives.contains(SERIALIZE));
  }

  #[test]
  fn test_deterministic_ordering() {
    let mut manager1 = DeriveManager::new(&[]);
    manager1.add(VALIDATE).add(DEBUG).add(SERIALIZE).add(CLONE);

    let mut manager2 = DeriveManager::new(&[]);
    manager2.add(CLONE).add(SERIALIZE).add(DEBUG).add(VALIDATE);

    let tokens1 = manager1.to_token_stream().to_string();
    let tokens2 = manager2.to_token_stream().to_string();

    assert_eq!(
      tokens1, tokens2,
      "Derive order should be deterministic regardless of insertion order"
    );
  }
}
