use crate::generator::codegen::{GeneratedResult, SchemaCodeGenerator};

pub trait GenerationMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult>;
}

pub struct TypesMode;

impl GenerationMode for TypesMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_types()
  }
}

pub struct ClientMode;

impl GenerationMode for ClientMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_client()
  }
}

pub struct ClientModMode;

impl GenerationMode for ClientModMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_client_mod()
  }
}

pub struct ServerModMode;

impl GenerationMode for ServerModMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_server_mod()
  }
}
