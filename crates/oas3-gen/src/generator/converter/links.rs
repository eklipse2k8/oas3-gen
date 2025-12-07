use std::collections::BTreeMap;

use oas3::{
  Spec,
  spec::{Link, ObjectOrReference, Response},
};

use super::runtime_expression;
use crate::generator::ast::LinkDef;

pub struct LinkConverter<'a> {
  spec: &'a Spec,
}

impl<'a> LinkConverter<'a> {
  pub fn new(spec: &'a Spec) -> Self {
    Self { spec }
  }

  pub fn extract_links_from_response(&self, response: &Response) -> Vec<LinkDef> {
    let mut links = Vec::new();

    for (name, link_ref) in &response.links {
      let link = match link_ref {
        ObjectOrReference::Object(link) => link,
        ObjectOrReference::Ref { ref_path, .. } => match self.resolve_link_ref(ref_path) {
          Some(resolved) => resolved,
          None => continue,
        },
      };

      let (operation_id, parameters_map, description, server_url) = match link {
        Link::Ref { .. } => {
          continue;
        }
        Link::Id {
          operation_id,
          parameters,
          description,
          server,
          ..
        } => (
          operation_id,
          parameters,
          description,
          server.as_ref().map(|s| s.url.clone()),
        ),
      };

      let mut parameters = BTreeMap::new();
      for (param_name, expr_str) in parameters_map {
        parameters.insert(param_name.clone(), runtime_expression::parse(expr_str));
      }

      links.push(LinkDef {
        name: name.clone(),
        target_operation_id: operation_id.clone(),
        parameters,
        description: description.clone(),
        server_url,
      });
    }

    links
  }

  fn resolve_link_ref(&self, ref_path: &str) -> Option<&Link> {
    let name = ref_path.strip_prefix("#/components/links/")?;
    self.spec.components.as_ref()?.links.get(name).and_then(|l| match l {
      ObjectOrReference::Object(link) => Some(link),
      ObjectOrReference::Ref { .. } => None,
    })
  }
}
