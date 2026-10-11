mod ast;
mod codegen;
mod converter;
mod naming;
mod operation_registry;
mod orchestrator;
mod postprocess;
mod schema_registry;

fn skeleton_spec(paths: serde_json::Value, components: serde_json::Value) -> serde_json::Result<oas3::Spec> {
  let mut document: serde_json::Value = serde_json::from_str(spec!("generator/skeleton.json"))?;
  document["paths"] = paths;
  document["components"] = components;
  serde_json::from_value(document)
}
