use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use oas3_gen::{
  generator::{
    ClientModMode, CodegenConfig, GenerationMode, GenerationTarget, HeaderScope, SchemaScope, ServerModMode,
    codegen::Visibility, orchestrator::Orchestrator,
  },
  utils::spec::SpecFormat,
};

const PETSTORE: &str = include_str!("../fixtures/petstore.json");

fn generated_bytes(target: GenerationTarget, mode: &dyn GenerationMode) -> usize {
  let spec = SpecFormat::Json
    .parse(PETSTORE.as_bytes())
    .expect("the petstore fixture parses");
  let config = CodegenConfig::builder()
    .target(target)
    .schema_scope(SchemaScope::All)
    .header_scope(HeaderScope::All)
    .enable_builders(true)
    .build();
  let output = Orchestrator::new(spec, Visibility::default(), config, None, None)
    .generate(mode, "petstore.json")
    .expect("the petstore fixture generates");
  output.code.code.values().map(String::len).sum()
}

fn petstore_pipeline(c: &mut Criterion) {
  let mut group = c.benchmark_group("petstore_pipeline");
  group.sample_size(20);
  group.bench_function("client_mod", |b| {
    b.iter(|| generated_bytes(black_box(GenerationTarget::Client), &ClientModMode::default()));
  });
  group.bench_function("server_mod", |b| {
    b.iter(|| generated_bytes(black_box(GenerationTarget::Server), &ServerModMode::default()));
  });
  group.finish();
}

criterion_group!(benches, petstore_pipeline);
criterion_main!(benches);
