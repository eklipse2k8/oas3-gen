mod multipart;
mod response;
mod serde_usage;

use anyhow::Context as _;
use oas3_gen::generator::{CodegenConfig, GenerationTarget, TypesMode, codegen::GeneratedFileType};
use quote::ToTokens;
use syn::{punctuated::Punctuated, token::Comma};

struct GeneratedFile {
  file: syn::File,
  warnings: Vec<String>,
}

impl GeneratedFile {
  fn types(spec_json: &str, target: GenerationTarget) -> anyhow::Result<Self> {
    let config = CodegenConfig::builder().target(target).build();
    let output =
      crate::make_orchestrator_with_config(oas3::from_json(spec_json)?, config).generate(&TypesMode, "spec.json")?;
    let code = output.code.code(&GeneratedFileType::Types).context("types output")?;
    Ok(Self {
      file: syn::parse_file(code)?,
      warnings: output.stats.warnings.iter().map(ToString::to_string).collect(),
    })
  }

  fn derives(&self, name: &str) -> anyhow::Result<Vec<String>> {
    let attrs = self
      .file
      .items
      .iter()
      .find_map(|item| match item {
        syn::Item::Struct(def) if def.ident == name => Some(&def.attrs),
        syn::Item::Enum(def) if def.ident == name => Some(&def.attrs),
        _ => None,
      })
      .with_context(|| format!("no type named {name}"))?;
    let mut derives = vec![];
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("derive")) {
      let paths = attr.parse_args_with(Punctuated::<syn::Path, Comma>::parse_terminated)?;
      derives.extend(paths.iter().map(tokens));
    }
    Ok(derives)
  }

  fn fields(&self, name: &str) -> anyhow::Result<Vec<String>> {
    let fields = self
      .file
      .items
      .iter()
      .find_map(|item| match item {
        syn::Item::Struct(def) if def.ident == name => Some(&def.fields),
        _ => None,
      })
      .with_context(|| format!("no struct named {name}"))?;
    Ok(
      fields
        .iter()
        .map(|field| format!("{}: {}", tokens(&field.ident), tokens(&field.ty)))
        .collect(),
    )
  }

  fn variants(&self, name: &str) -> anyhow::Result<Vec<String>> {
    let variants = self
      .file
      .items
      .iter()
      .find_map(|item| match item {
        syn::Item::Enum(def) if def.ident == name => Some(&def.variants),
        _ => None,
      })
      .with_context(|| format!("no enum named {name}"))?;
    Ok(
      variants
        .iter()
        .map(|variant| format!("{}{}", variant.ident, tokens(&variant.fields)))
        .collect(),
    )
  }

  fn parse_response_methods(&self) -> impl Iterator<Item = (String, &syn::ImplItemFn)> {
    self.file.items.iter().filter_map(|item| {
      let syn::Item::Impl(block) = item else {
        return None;
      };
      block.items.iter().find_map(|member| match member {
        syn::ImplItem::Fn(method) if method.sig.ident == "parse_response" => Some((tokens(&block.self_ty), method)),
        _ => None,
      })
    })
  }

  fn response_types(&self) -> Vec<String> {
    self
      .parse_response_methods()
      .map(|(request, method)| format!("{request} -> {}", result_value(&method.sig.output)))
      .collect()
  }

  fn dispatch(&self, request: &str) -> anyhow::Result<Vec<String>> {
    let (_, method) = self
      .parse_response_methods()
      .find(|(name, _)| name == request)
      .with_context(|| format!("no parse_response on {request}"))?;
    Ok(
      method
        .block
        .stmts
        .iter()
        .filter_map(|stmt| match stmt {
          syn::Stmt::Expr(syn::Expr::If(branch), _) => Some(tokens(&branch.cond)),
          _ => None,
        })
        .collect(),
    )
  }
}

fn result_value(output: &syn::ReturnType) -> String {
  let syn::ReturnType::Type(_, returned) = output else {
    return String::new();
  };
  let syn::Type::Path(result) = returned.as_ref() else {
    return tokens(returned);
  };
  match result.path.segments.last().map(|segment| &segment.arguments) {
    Some(syn::PathArguments::AngleBracketed(arguments)) => arguments.args.first().map_or_else(String::new, tokens),
    _ => tokens(result),
  }
}

fn tokens(node: &impl ToTokens) -> String {
  node.to_token_stream().to_string().split_whitespace().collect()
}
