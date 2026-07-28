use crate::generator::codegen::{CratePackage, GeneratedResult, SchemaCodeGenerator};

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

#[derive(Debug, Default)]
pub struct ClientModMode {
  package: Option<CratePackage>,
}

impl ClientModMode {
  #[must_use]
  pub fn with_package(package: Option<CratePackage>) -> Self {
    Self { package }
  }
}

impl GenerationMode for ClientModMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_client_mod(self.package.as_ref())
  }
}

#[derive(Debug, Default)]
pub struct ServerModMode {
  package: Option<CratePackage>,
}

impl ServerModMode {
  #[must_use]
  pub fn with_package(package: Option<CratePackage>) -> Self {
    Self { package }
  }
}

impl GenerationMode for ServerModMode {
  fn generate(&self, codegen: &SchemaCodeGenerator) -> anyhow::Result<GeneratedResult> {
    codegen.generate_server_mod(self.package.as_ref())
  }
}
