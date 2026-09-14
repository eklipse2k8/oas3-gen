use std::{
  collections::BTreeSet,
  ffi::OsString,
  fmt::Write as _,
  path::{Component, Path},
};

use anyhow::Context as _;
use inflections::Inflect as _;
use itertools::Itertools as _;
use proc_macro2::{Spacing, TokenStream, TokenTree};
use serde::Serialize;
use toml::ser::ValueSerializer;

use super::mod_file::ModFileKind;
use crate::generator::{converter::CollectionTypePolicy, naming::identifiers::sanitize};

const PACKAGE_EDITION: &str = "2024";
const PACKAGE_RUST_VERSION: &str = "1.89";

/// A dependency the generated code may reference, pinned to the version the
/// generator itself is built and tested against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DependencySpec {
  ident: &'static str,
  package: &'static str,
  version: &'static str,
  features: &'static [&'static str],
  ordered_features: &'static [&'static str],
  default_features: bool,
}

const DEPENDENCIES: &[DependencySpec] = &[
  DependencySpec {
    ident: "anyhow",
    package: "anyhow",
    version: "1.0",
    features: &[],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "axum",
    package: "axum",
    version: "0.8",
    features: &[],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "bon",
    package: "bon",
    version: "3.10",
    features: &["implied-bounds"],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "chrono",
    package: "chrono",
    version: "0.4.42",
    features: &["std", "clock", "serde"],
    ordered_features: &[],
    default_features: false,
  },
  DependencySpec {
    ident: "http",
    package: "http",
    version: "1.4",
    features: &[],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "indexmap",
    package: "indexmap",
    version: "2.14",
    features: &["serde"],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "oas3_gen_support",
    package: "oas3-gen-support",
    version: env!("CARGO_PKG_VERSION"),
    features: &[],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "regex",
    package: "regex",
    version: "1.13",
    features: &[],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "reqwest",
    package: "reqwest",
    version: "0.13",
    features: &["json", "multipart", "http2", "native-tls", "query", "stream"],
    ordered_features: &[],
    default_features: false,
  },
  DependencySpec {
    ident: "serde",
    package: "serde",
    version: "1.0",
    features: &["derive"],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "serde_json",
    package: "serde_json",
    version: "1.0",
    features: &[],
    ordered_features: &["preserve_order"],
    default_features: true,
  },
  DependencySpec {
    ident: "serde_with",
    package: "serde_with",
    version: "3.23",
    features: &["base64", "chrono"],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "uuid",
    package: "uuid",
    version: "1.26",
    features: &["serde"],
    ordered_features: &[],
    default_features: true,
  },
  DependencySpec {
    ident: "validator",
    package: "validator",
    version: "0.21",
    features: &["derive"],
    ordered_features: &[],
    default_features: true,
  },
];

/// Yields the pinned `(package, version)` pairs the generated manifests declare.
#[cfg(test)]
pub(crate) fn pinned_versions() -> impl Iterator<Item = (&'static str, &'static str)> {
  DEPENDENCIES.iter().map(|spec| (spec.package, spec.version))
}

impl DependencySpec {
  fn resolve(&self, collection_types: CollectionTypePolicy) -> Dependency {
    let features = match collection_types {
      CollectionTypePolicy::Ordered => self
        .features
        .iter()
        .chain(self.ordered_features)
        .copied()
        .collect::<Vec<_>>(),
      CollectionTypePolicy::Hashed => self.features.to_vec(),
    };

    let value = if features.is_empty() && self.default_features {
      DependencyValue::Version(self.version)
    } else {
      DependencyValue::Detailed(DetailedDependency {
        version: self.version,
        default_features: (!self.default_features).then_some(false),
        features,
      })
    };

    Dependency {
      package: self.package,
      value,
    }
  }
}

/// A resolved `[dependencies]` entry ready to be written to a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dependency {
  package: &'static str,
  value: DependencyValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
enum DependencyValue {
  Version(&'static str),
  Detailed(DetailedDependency),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct DetailedDependency {
  version: &'static str,
  #[serde(rename = "default-features", skip_serializing_if = "Option::is_none")]
  default_features: Option<bool>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  features: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ManifestDocument<'a> {
  package: PackageSection<'a>,
}

#[derive(Debug, Serialize)]
struct PackageSection<'a> {
  name: &'a str,
  version: &'a str,
  edition: &'static str,
  #[serde(rename = "rust-version")]
  rust_version: &'static str,
  description: &'a str,
}

/// Cargo package metadata (name and version) for a generated crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CratePackage {
  name: String,
  version: String,
}

impl CratePackage {
  /// Derives a package name from the final component of an output directory.
  ///
  /// # Errors
  ///
  /// Returns an error when the path has no usable final component, or when the
  /// sanitized name is empty or starts with a digit.
  pub fn from_output_dir(path: &Path, version: String) -> anyhow::Result<Self> {
    let directory =
      directory_name(path).with_context(|| format!("deriving a crate name from output path '{}'", path.display()))?;
    Self::new(&directory, version)
  }

  /// Creates a package from a raw name, sanitizing it into a valid cargo package name.
  ///
  /// # Errors
  ///
  /// Returns an error when sanitization yields an empty name or a name that
  /// starts with a digit.
  pub fn new(raw: &str, version: String) -> anyhow::Result<Self> {
    let name = sanitize(raw).to_kebab_case();
    anyhow::ensure!(!name.is_empty(), "cannot derive a crate name from '{raw}'");
    anyhow::ensure!(
      !name.starts_with(|first: char| first.is_ascii_digit()),
      "crate name '{name}' derived from '{raw}' cannot start with a digit"
    );
    Ok(Self { name, version })
  }

  #[must_use]
  pub fn name(&self) -> &str {
    &self.name
  }

  #[must_use]
  pub fn version(&self) -> &str {
    &self.version
  }
}

/// A `Cargo.toml` for a generated module compiled as its own crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoManifest {
  package: CratePackage,
  description: String,
  generator_version: String,
  dependencies: Vec<Dependency>,
}

impl CargoManifest {
  /// Builds a manifest whose dependency set is derived from the crate paths
  /// referenced by the generated sources.
  ///
  /// # Errors
  ///
  /// Returns an error when a generated source cannot be tokenized.
  pub fn new(
    package: CratePackage,
    kind: ModFileKind,
    title: &str,
    generator_version: String,
    collection_types: CollectionTypePolicy,
    sources: &[&str],
  ) -> anyhow::Result<Self> {
    let referenced = referenced_crates(sources)?;
    let dependencies = DEPENDENCIES
      .iter()
      .filter(|spec| referenced.contains(spec.ident))
      .map(|spec| spec.resolve(collection_types))
      .collect::<Vec<_>>();

    Ok(Self {
      package,
      description: format!(
        "Rust {} generated from the {} OpenAPI document",
        kind.label(),
        title.split_whitespace().join(" ")
      ),
      generator_version,
      dependencies,
    })
  }

  /// Renders the manifest as TOML, with the package table and every dependency
  /// value serialized by the `toml` crate.
  ///
  /// # Errors
  ///
  /// Returns an error when the package metadata or a dependency value cannot be
  /// serialized.
  pub fn render(&self) -> anyhow::Result<String> {
    let package = toml::to_string(&ManifestDocument {
      package: PackageSection {
        name: self.package.name(),
        version: self.package.version(),
        edition: PACKAGE_EDITION,
        rust_version: PACKAGE_RUST_VERSION,
        description: &self.description,
      },
    })
    .context("serializing generated package metadata")?;

    let mut manifest = format!(
      "# AUTO-GENERATED CODE - DO NOT EDIT!\n\
       # Generated by `oas3-gen v{}`\n\
       \n\
       {package}\n\
       [dependencies]\n",
      self.generator_version
    );

    for dependency in &self.dependencies {
      let mut value = String::new();
      dependency
        .value
        .serialize(ValueSerializer::new(&mut value))
        .with_context(|| format!("serializing dependency '{}'", dependency.package))?;
      writeln!(manifest, "{} = {value}", dependency.package)?;
    }

    Ok(manifest)
  }
}

fn directory_name(path: &Path) -> Option<String> {
  let absolute = std::path::absolute(path).ok()?;
  let mut segments: Vec<OsString> = vec![];

  for component in absolute.components() {
    match component {
      Component::Normal(segment) => segments.push(segment.to_os_string()),
      Component::ParentDir => {
        segments.pop();
      }
      _ => {}
    }
  }

  segments.pop().map(|segment| segment.to_string_lossy().into_owned())
}

fn referenced_crates(sources: &[&str]) -> anyhow::Result<BTreeSet<String>> {
  let mut roots = BTreeSet::new();
  for source in sources {
    let tokens = source
      .parse::<TokenStream>()
      .map_err(|error| anyhow::anyhow!("{error}"))
      .context("tokenizing generated source for dependency detection")?;
    collect_path_roots(tokens, &mut roots);
  }
  Ok(roots)
}

fn collect_path_roots(tokens: TokenStream, roots: &mut BTreeSet<String>) {
  let mut previous_ident: Option<String> = None;

  for token in tokens {
    match token {
      TokenTree::Ident(ident) => previous_ident = Some(ident.to_string()),
      TokenTree::Punct(punct) if punct.as_char() == ':' && punct.spacing() == Spacing::Joint => {
        if let Some(ident) = previous_ident.take() {
          roots.insert(ident);
        }
      }
      TokenTree::Group(group) => {
        previous_ident = None;
        collect_path_roots(group.stream(), roots);
      }
      _ => previous_ident = None,
    }
  }
}
