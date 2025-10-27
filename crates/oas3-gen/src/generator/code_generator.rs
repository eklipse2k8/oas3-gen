use std::collections::BTreeMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::ast::*;
use crate::reserved::{header_const_name, regex_const_name};

/// Visibility level for generated types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
  /// Public visibility (`pub`)
  #[default]
  Public,
  /// Crate visibility (`pub(crate)`)
  Crate,
  /// File/module-private (no visibility modifier)
  File,
}

impl Visibility {
  /// Parse a string into a Visibility
  pub fn parse(s: &str) -> Option<Self> {
    match s {
      "public" => Some(Visibility::Public),
      "crate" => Some(Visibility::Crate),
      "file" => Some(Visibility::File),
      _ => None,
    }
  }

  /// Get the Rust visibility token
  fn to_tokens(self) -> TokenStream {
    match self {
      Visibility::Public => quote! { pub },
      Visibility::Crate => quote! { pub(crate) },
      Visibility::File => quote! {},
    }
  }
}

/// Type usage context for determining derive traits
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeUsage {
  /// Type is only used in requests (needs Serialize)
  RequestOnly,
  /// Type is only used in responses (needs Deserialize)
  ResponseOnly,
  /// Type is used in both requests and responses (needs both)
  Bidirectional,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RegexKey {
  type_name: String,
  variant_name: Option<String>,
  field_name: String,
}

impl RegexKey {
  fn for_struct(type_name: &str, field_name: &str) -> Self {
    Self {
      type_name: type_name.to_string(),
      variant_name: None,
      field_name: field_name.to_string(),
    }
  }

  fn for_variant(type_name: &str, variant_name: &str, field_name: &str) -> Self {
    Self {
      type_name: type_name.to_string(),
      variant_name: Some(variant_name.to_string()),
      field_name: field_name.to_string(),
    }
  }

  fn parts(&self) -> Vec<&str> {
    let mut parts = vec![self.type_name.as_str()];
    if let Some(variant) = &self.variant_name {
      parts.push(variant.as_str());
    }
    parts.push(self.field_name.as_str());
    parts
  }
}

pub(crate) struct CodeGenerator;

impl CodeGenerator {
  fn ensure_derive(derives: &mut Vec<String>, trait_name: &str) {
    if !derives.iter().any(|existing| existing == trait_name) {
      derives.push(trait_name.to_string());
    }
  }

  /// Build a map of type usage (request/response) from operations
  pub(crate) fn build_type_usage_map(operations: &[OperationInfo]) -> BTreeMap<String, TypeUsage> {
    let mut usage_map: BTreeMap<String, (bool, bool)> = BTreeMap::new();

    for op in operations {
      if let Some(ref req_type) = op.request_type {
        let entry = usage_map.entry(req_type.clone()).or_insert((false, false));
        entry.0 = true;
      }

      for body_type in &op.request_body_types {
        let entry = usage_map.entry(body_type.clone()).or_insert((false, false));
        entry.0 = true;
      }

      if let Some(ref resp_type) = op.response_type {
        let entry = usage_map.entry(resp_type.clone()).or_insert((false, false));
        entry.1 = true;
      }
    }

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

  pub(crate) fn generate(
    types: &[RustType],
    type_usage: &BTreeMap<String, TypeUsage>,
    headers: Vec<&String>,
    visibility: Visibility,
  ) -> TokenStream {
    let ordered = Self::ordered_types(types);
    let (regex_consts, regex_lookup) = Self::generate_regex_constants(&ordered);
    let header_consts = Self::generate_header_constants(headers);
    let type_tokens: Vec<TokenStream> = ordered
      .iter()
      .map(|ty| Self::generate_type(ty, &regex_lookup, type_usage, visibility))
      .collect();

    quote! {
      use serde::{Deserialize, Serialize};

      #regex_consts

      #header_consts

      #(#type_tokens)*
    }
  }

  fn ordered_types<'a>(types: &'a [RustType]) -> Vec<&'a RustType> {
    let mut map: BTreeMap<String, &'a RustType> = BTreeMap::new();
    for ty in types {
      let name = ty.type_name().to_string();

      // If name already exists, keep the one with higher priority
      if let Some(existing) = map.get(&name) {
        let existing_priority = Self::type_priority(existing);
        let new_priority = Self::type_priority(ty);

        // Keep the one with higher priority (lower number = higher priority)
        if new_priority < existing_priority {
          map.insert(name, ty);
        }
      } else {
        map.insert(name, ty);
      }
    }
    map.into_values().collect()
  }

  /// Determine type priority for deduplication (lower = higher priority)
  /// Priority: DiscriminatedEnum > Enum > Struct > TypeAlias
  fn type_priority(rust_type: &RustType) -> u8 {
    match rust_type {
      RustType::DiscriminatedEnum(_) => 0, // Highest - references other types
      RustType::Enum(_) => 1,              // High priority - most specific
      RustType::Struct(_) => 2,            // Medium priority
      RustType::TypeAlias(_) => 3,         // Lowest priority
    }
  }

  /// Generate regex constants for validation
  fn generate_regex_constants(types: &[&RustType]) -> (TokenStream, BTreeMap<RegexKey, String>) {
    let mut const_defs: BTreeMap<String, String> = BTreeMap::new();
    let mut lookup: BTreeMap<RegexKey, String> = BTreeMap::new();
    let mut pattern_to_const: BTreeMap<String, String> = BTreeMap::new();

    for rust_type in types {
      match rust_type {
        RustType::Struct(def) => {
          for field in &def.fields {
            let Some(pattern) = &field.regex_validation else {
              continue;
            };
            let key = RegexKey::for_struct(&def.name, &field.name);
            let pattern_key = pattern.clone();
            let const_name = match pattern_to_const.get(&pattern_key) {
              Some(existing) => existing.clone(),
              None => {
                let name = regex_const_name(&key.parts());
                pattern_to_const.insert(pattern_key.clone(), name.clone());
                const_defs.insert(name.clone(), pattern_key);
                name
              }
            };
            lookup.insert(key, const_name);
          }
        }
        RustType::Enum(def) => {
          for variant in &def.variants {
            if let VariantContent::Struct(fields) = &variant.content {
              for field in fields {
                let Some(pattern) = &field.regex_validation else {
                  continue;
                };
                let key = RegexKey::for_variant(&def.name, &variant.name, &field.name);
                let pattern_key = pattern.clone();
                let const_name = match pattern_to_const.get(&pattern_key) {
                  Some(existing) => existing.clone(),
                  None => {
                    let name = regex_const_name(&key.parts());
                    pattern_to_const.insert(pattern_key.clone(), name.clone());
                    const_defs.insert(name.clone(), pattern_key);
                    name
                  }
                };
                lookup.insert(key, const_name);
              }
            }
          }
        }
        RustType::TypeAlias(_) => {}
        RustType::DiscriminatedEnum(_) => {}
      }
    }

    if const_defs.is_empty() {
      return (quote! {}, lookup);
    }

    let regex_defs: Vec<TokenStream> = const_defs
      .into_iter()
      .map(|(name, pattern)| {
        let ident = format_ident!("{}", name);
        quote! {
          static #ident: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(#pattern).expect("invalid regex"));
        }
      })
      .collect();

    (quote! { #(#regex_defs)* }, lookup)
  }

  /// Generate HTTP header name constants from collected headers
  fn generate_header_constants(headers: Vec<&String>) -> TokenStream {
    if headers.is_empty() {
      return quote! {};
    }

    let const_tokens: Vec<TokenStream> = headers
      .iter()
      .map(|header| {
        let const_name = header_const_name(header);
        let ident = format_ident!("{}", const_name);
        quote! {
          pub const #ident: http::HeaderName = http::HeaderName::from_static(#header);
        }
      })
      .collect();

    quote! { #(#const_tokens)* }
  }

  /// Convert a JSON value to a Rust expression
  fn json_value_to_rust_expr(value: &serde_json::Value, rust_type: &TypeRef) -> TokenStream {
    let base_expr = match value {
      serde_json::Value::String(s) => {
        quote! { #s.to_string() }
      }
      serde_json::Value::Number(n) => {
        if let Some(i) = n.as_i64() {
          quote! { #i }
        } else if let Some(f) = n.as_f64() {
          quote! { #f }
        } else {
          quote! { Default::default() }
        }
      }
      serde_json::Value::Bool(b) => {
        quote! { #b }
      }
      serde_json::Value::Null => {
        quote! { None }
      }
      serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
        quote! { Default::default() }
      }
    };

    if rust_type.nullable && !matches!(value, serde_json::Value::Null) {
      quote! { Some(#base_expr) }
    } else {
      base_expr
    }
  }

  fn generate_type(
    rust_type: &RustType,
    regex_lookup: &BTreeMap<RegexKey, String>,
    type_usage: &BTreeMap<String, TypeUsage>,
    visibility: Visibility,
  ) -> TokenStream {
    match rust_type {
      RustType::Struct(def) => Self::generate_struct(def, regex_lookup, type_usage, visibility),
      RustType::Enum(def) => Self::generate_enum(def, regex_lookup, visibility),
      RustType::TypeAlias(def) => Self::generate_type_alias(def, visibility),
      RustType::DiscriminatedEnum(def) => Self::generate_discriminated_enum(def, visibility),
    }
  }

  fn generate_struct(
    def: &StructDef,
    regex_lookup: &BTreeMap<RegexKey, String>,
    type_usage: &BTreeMap<String, TypeUsage>,
    visibility: Visibility,
  ) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let vis = visibility.to_tokens();

    let is_operation_type = def.name.ends_with("Request") || def.name.ends_with("RequestBody");

    let derives = if is_operation_type && type_usage.contains_key(&def.name) {
      let usage = type_usage.get(&def.name).unwrap();
      let mut custom = vec!["Debug".to_string(), "Clone".to_string(), "PartialEq".to_string()];
      match usage {
        TypeUsage::RequestOnly => {
          custom.push("Serialize".to_string());
          custom.push("validator::Validate".to_string());
        }
        TypeUsage::ResponseOnly => {
          custom.push("Deserialize".to_string());
        }
        TypeUsage::Bidirectional => {
          custom.push("Serialize".to_string());
          custom.push("Deserialize".to_string());
          custom.push("validator::Validate".to_string());
        }
      }
      custom.push("oas3_gen_support::Default".to_string());
      Self::generate_derives(&custom)
    } else {
      let mut derives = def.derives.clone();
      if let Some(usage) = type_usage.get(&def.name) {
        match usage {
          TypeUsage::RequestOnly => {
            Self::ensure_derive(&mut derives, "Serialize");
            Self::ensure_derive(&mut derives, "validator::Validate");
          }
          TypeUsage::ResponseOnly => {
            Self::ensure_derive(&mut derives, "Deserialize");
          }
          TypeUsage::Bidirectional => {
            Self::ensure_derive(&mut derives, "Serialize");
            Self::ensure_derive(&mut derives, "Deserialize");
            Self::ensure_derive(&mut derives, "validator::Validate");
          }
        }
      }
      Self::generate_derives(&derives)
    };

    let outer_attrs = Self::generate_outer_attrs(&def.outer_attrs);
    let serde_attrs = Self::generate_serde_attrs(&def.serde_attrs);

    let include_validation = !(is_operation_type && matches!(type_usage.get(&def.name), Some(TypeUsage::ResponseOnly)));
    let fields = Self::generate_fields_with_visibility(
      &def.name,
      None,
      &def.fields,
      true,
      include_validation,
      regex_lookup,
      visibility,
    );

    let struct_tokens = quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis struct #name {
        #(#fields),*
      }
    };

    if def.methods.is_empty() {
      struct_tokens
    } else {
      let methods: Vec<TokenStream> = def.methods.iter().map(Self::generate_struct_method).collect();

      quote! {
        #struct_tokens

        impl #name {
          #(#methods)*
        }
      }
    }
  }

  fn generate_struct_method(method: &StructMethod) -> TokenStream {
    let docs = Self::generate_docs(&method.docs);
    let name = format_ident!("{}", method.name);
    let body = match &method.kind {
      StructMethodKind::RenderPath { segments, query_params } => {
        let mut format_string = String::new();
        let mut fallback_string = String::new();
        let mut args: Vec<TokenStream> = Vec::new();

        for segment in segments {
          match segment {
            PathSegment::Literal(lit) => {
              let escaped = lit.replace('{', "{{").replace('}', "}}");
              format_string.push_str(&escaped);
              fallback_string.push_str(lit);
            }
            PathSegment::Parameter { field } => {
              format_string.push_str("{}");
              fallback_string.push_str("{}");
              let ident = format_ident!("{}", field);
              args.push(quote! {
                oas3_gen_support::percent_encode_path_segment(&self.#ident.to_string())
              });
            }
          }
        }

        let path_expr = if args.is_empty() {
          quote! { #fallback_string.to_string() }
        } else {
          let args_tokens = args;
          quote! { format!(#format_string, #(#args_tokens),*) }
        };

        if query_params.is_empty() {
          path_expr
        } else {
          let query_statements: Vec<TokenStream> =
            query_params.iter().map(Self::generate_query_param_statement).collect();

          quote! {
            use std::fmt::Write as _;
            let mut path = #path_expr;
            let mut prefix = '\0';
            #(#query_statements)*
            path
          }
        }
      }
    };

    quote! {
      #docs
      pub fn #name(&self) -> String {
        #body
      }
    }
  }

  fn generate_query_param_statement(param: &QueryParameter) -> TokenStream {
    let ident = format_ident!("{}", param.field);
    let key = &param.encoded_name;
    let param_equal = format!("{{prefix}}{key}={{}}");

    if param.optional {
      if param.is_array {
        if param.explode {
          quote! {
            if let Some(values) = &self.#ident {
              for value in values {
                prefix = if prefix != '\0' { '&' } else { '?' };
                write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&value.to_string())).unwrap();
              }
            }
          }
        } else {
          quote! {
            if let Some(values) = &self.#ident {
              if !values.is_empty() {
                prefix = if prefix != '\0' { '&' } else { '?' };
                let values = values.iter().map(|v| oas3_gen_support::percent_encode_query_component(&v)).collect::<Vec<_>>().join(",");
                write!(&mut path, #param_equal, values).unwrap();
              }
            }
          }
        }
      } else {
        quote! {
          if let Some(value) = &self.#ident {
            prefix = if prefix != '\0' { '&' } else { '?' };
            write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&value.to_string())).unwrap();
          }
        }
      }
    } else if param.is_array {
      if param.explode {
        quote! {
          for value in &self.#ident {
            prefix = if prefix != '\0' { '&' } else { '?' };
            write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&value.to_string())).unwrap();
          }
        }
      } else {
        quote! {
          if !self.#ident.is_empty() {
            prefix = if prefix != '\0' { '&' } else { '?' };
            let values = self.#ident.iter().map(|v| oas3_gen_support::percent_encode_query_component(&v)).collect::<Vec<_>>().join(",");
            write!(&mut path, #param_equal, values).unwrap();
          }
        }
      }
    } else {
      quote! {
        prefix = if prefix != '\0' { '&' } else { '?' };
        write!(&mut path, #param_equal, oas3_gen_support::percent_encode_query_component(&self.#ident.to_string())).unwrap();
      }
    }
  }

  fn generate_enum(def: &EnumDef, regex_lookup: &BTreeMap<RegexKey, String>, visibility: Visibility) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let vis = visibility.to_tokens();
    let derives = Self::generate_derives(&def.derives);
    let outer_attrs = Self::generate_outer_attrs(&def.outer_attrs);
    let serde_attrs = Self::generate_enum_serde_attrs(def);
    let variants = Self::generate_variants(&def.name, &def.variants, regex_lookup);

    quote! {
      #docs
      #outer_attrs
      #derives
      #serde_attrs
      #vis enum #name {
        #(#variants),*
      }
    }
  }

  fn generate_type_alias(def: &TypeAliasDef, visibility: Visibility) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let docs = Self::generate_docs(&def.docs);
    let vis = visibility.to_tokens();
    let target = Self::parse_type_string(&def.target.to_rust_type());

    quote! {
      #docs
      #vis type #name = #target;
    }
  }

  fn generate_discriminated_enum(
    def: &crate::generator::ast::DiscriminatedEnumDef,
    visibility: Visibility,
  ) -> TokenStream {
    let name = format_ident!("{}", def.name);
    let disc_field = &def.discriminator_field;
    let docs = Self::generate_docs(&def.docs);
    let vis = visibility.to_tokens();

    let variants: Vec<TokenStream> = def
      .variants
      .iter()
      .map(|v| {
        let disc_value = &v.discriminator_value;
        let variant_name = format_ident!("{}", v.variant_name);
        let type_name = Self::parse_type_string(&v.type_name);
        quote! { (#disc_value, #variant_name(#type_name)) }
      })
      .collect();

    if let Some(ref fallback) = def.fallback {
      let fallback_variant = format_ident!("{}", fallback.variant_name);
      let fallback_type = Self::parse_type_string(&fallback.type_name);

      quote! {
        #docs
        oas3_gen_support::discriminated_enum! {
          #vis enum #name {
            discriminator: #disc_field,
            variants: [
              #(#variants),*
            ],
            fallback: #fallback_variant(#fallback_type),
          }
        }
      }
    } else {
      quote! {
        #docs
        oas3_gen_support::discriminated_enum! {
          #vis enum #name {
            discriminator: #disc_field,
            variants: [
              #(#variants),*
            ],
          }
        }
      }
    }
  }

  fn generate_docs(docs: &[String]) -> TokenStream {
    if docs.is_empty() {
      return quote! {};
    }
    let doc_lines: Vec<TokenStream> = docs
      .iter()
      .map(|line| {
        let clean = line.strip_prefix("/// ").unwrap_or(line);
        quote! { #[doc = #clean] }
      })
      .collect();
    quote! { #(#doc_lines)* }
  }

  fn generate_derives(derives: &[String]) -> TokenStream {
    if derives.is_empty() {
      return quote! {};
    }
    let derive_idents = derives
      .iter()
      .map(|d| syn::parse_str(d).unwrap_or_else(|_| quote! {}))
      .collect::<Vec<_>>();

    quote! { #[derive(#(#derive_idents),*)] }
  }

  fn generate_outer_attrs(attrs: &[String]) -> TokenStream {
    if attrs.is_empty() {
      return quote! {};
    }
    let attr_tokens: Vec<TokenStream> = attrs
      .iter()
      .map(|attr| {
        let trimmed = attr.trim();
        if trimmed.is_empty() {
          return quote! {};
        }
        let source = if trimmed.starts_with("#[") {
          trimmed.to_string()
        } else {
          format!("#[{}]", trimmed)
        };
        syn::parse_str::<TokenStream>(&source).unwrap_or_else(|_| quote! {})
      })
      .collect();
    quote! { #(#attr_tokens)* }
  }

  fn generate_serde_attrs(attrs: &[String]) -> TokenStream {
    if attrs.is_empty() {
      return quote! {};
    }
    let attr_tokens: Vec<TokenStream> = attrs
      .iter()
      .map(|attr| {
        let tokens: TokenStream = attr.as_str().parse().unwrap_or_else(|_| quote! {});
        quote! { #[serde(#tokens)] }
      })
      .collect();
    quote! { #(#attr_tokens)* }
  }

  fn generate_validation_attrs(regex_const: Option<&str>, attrs: &[String]) -> TokenStream {
    if attrs.is_empty() && regex_const.is_none() {
      return quote! {};
    }

    let mut combined = attrs.to_owned();

    if let Some(const_name) = regex_const {
      combined.push(format!("regex(path = \"{}\")", const_name));
    }

    let attr_tokens: Vec<TokenStream> = combined
      .iter()
      .map(|attr| attr.parse().unwrap_or_else(|_| quote! {}))
      .collect();

    quote! { #[validate(#(#attr_tokens),*)] }
  }

  fn generate_enum_serde_attrs(def: &EnumDef) -> TokenStream {
    let mut attrs = Vec::new();

    // Add discriminator tag if present
    if let Some(ref discriminator) = def.discriminator {
      attrs.push(quote! { tag = #discriminator });
    }

    // Add other serde attributes
    for attr in &def.serde_attrs {
      if let Ok(tokens) = attr.parse::<TokenStream>() {
        attrs.push(tokens);
      }
    }

    if attrs.is_empty() {
      return quote! {};
    }

    quote! {
      #[serde(#(#attrs),*)]
    }
  }

  fn generate_fields_with_visibility(
    type_name: &str,
    variant_name: Option<&str>,
    fields: &[FieldDef],
    add_pub: bool,
    include_validation: bool,
    regex_lookup: &BTreeMap<RegexKey, String>,
    visibility: Visibility,
  ) -> Vec<TokenStream> {
    fields
      .iter()
      .map(|field| {
        let name = format_ident!("{}", field.name);

        // Add validation constraints to docs
        let mut field_docs = field.docs.clone();
        if let Some(ref multiple_of) = field.multiple_of {
          field_docs.push(format!("/// Validation: Must be a multiple of {}", multiple_of));
        }

        let docs = Self::generate_docs(&field_docs);
        let serde_attrs = Self::generate_serde_attrs(&field.serde_attrs);
        let extra_attrs: Vec<TokenStream> = field
          .extra_attrs
          .iter()
          .filter_map(|attr| attr.parse::<TokenStream>().ok())
          .collect();

        // Only include validation for struct fields, not enum variant fields
        let regex_const = if include_validation && field.regex_validation.is_some() {
          let key = match variant_name {
            Some(variant) => RegexKey::for_variant(type_name, variant, &field.name),
            None => RegexKey::for_struct(type_name, &field.name),
          };
          regex_lookup.get(&key).map(|s| s.as_str())
        } else {
          None
        };

        let validation_attrs = if include_validation {
          Self::generate_validation_attrs(regex_const, &field.validation_attrs)
        } else {
          quote! {}
        };

        let deprecated_attr = if field.deprecated {
          quote! { #[deprecated] }
        } else {
          quote! {}
        };

        // Generate #[default(value)] attribute for fields with default values
        let default_attr = if add_pub && field.default_value.is_some() {
          let default_expr = Self::json_value_to_rust_expr(field.default_value.as_ref().unwrap(), &field.rust_type);
          quote! { #[default(#default_expr)] }
        } else {
          quote! {}
        };

        let type_tokens = Self::parse_type_string(&field.rust_type.to_rust_type());

        if add_pub {
          let vis = visibility.to_tokens();
          quote! {
            #(#extra_attrs)*
            #docs
            #deprecated_attr
            #serde_attrs
            #validation_attrs
            #default_attr
            #vis #name: #type_tokens
          }
        } else {
          quote! {
            #(#extra_attrs)*
            #docs
            #deprecated_attr
            #serde_attrs
            #validation_attrs
            #name: #type_tokens
          }
        }
      })
      .collect()
  }

  fn generate_variants(
    type_name: &str,
    variants: &[VariantDef],
    regex_lookup: &BTreeMap<RegexKey, String>,
  ) -> Vec<TokenStream> {
    variants
      .iter()
      .enumerate()
      .map(|(idx, variant)| {
        let name = format_ident!("{}", variant.name);
        let docs = Self::generate_docs(&variant.docs);
        let serde_attrs = Self::generate_serde_attrs(&variant.serde_attrs);

        let deprecated_attr = if variant.deprecated {
          quote! { #[deprecated] }
        } else {
          quote! {}
        };

        // Add #[default] to the first variant for Rust's built-in Default derive
        let default_attr = if idx == 0 {
          quote! { #[default] }
        } else {
          quote! {}
        };

        let content = match &variant.content {
          VariantContent::Unit => quote! {},
          VariantContent::Tuple(types) => {
            let type_tokens: Vec<_> = types
              .iter()
              .map(|t| Self::parse_type_string(&t.to_rust_type()))
              .collect();
            quote! { ( #(#type_tokens),* ) }
          }
          VariantContent::Struct(fields) => {
            // Enum variant fields should not have 'pub' keyword or validation attributes
            // Use File visibility for enum variant fields (they never have visibility modifiers)
            let field_tokens = Self::generate_fields_with_visibility(
              type_name,
              Some(&variant.name),
              fields,
              false,
              false,
              regex_lookup,
              Visibility::File,
            );
            quote! { { #(#field_tokens),* } }
          }
        };

        quote! {
          #docs
          #deprecated_attr
          #serde_attrs
          #default_attr
          #name #content
        }
      })
      .collect()
  }

  /// Parse a type string into a TokenStream
  /// This is a simple parser that handles basic Rust types
  fn parse_type_string(type_str: &str) -> TokenStream {
    // For now, just parse it directly - this works for most cases
    type_str.parse().unwrap_or_else(|_| quote! { serde_json::Value })
  }
}
