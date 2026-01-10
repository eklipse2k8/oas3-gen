#[cfg(feature = "mdformat")]
use std::process::Stdio;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
#[cfg(feature = "mdformat")]
use tokio::{process::Command, runtime::Handle};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Documentation {
  lines: Vec<String>,
}

#[bon::bon]
impl Documentation {
  #[builder]
  pub(crate) fn documentation(
    summary: Option<&str>,
    description: Option<&str>,
    method: &http::Method,
    path: Option<&str>,
  ) -> Self {
    let mut docs = Self::default();

    if let Some(s) = summary {
      for line in s.lines().filter(|l| !l.trim().is_empty()) {
        docs.push(line.trim().to_string());
      }
    }

    if let Some(desc) = description {
      if summary.is_some() {
        docs.push(String::new());
      }
      for line in desc.lines() {
        docs.push(line.trim().to_string());
      }
    }

    if summary.is_some() || description.is_some() {
      docs.push(String::new());
    }

    if let Some(p) = path {
      docs.push("## Endpoint".to_string());
      docs.push(format!("{} {}", method.as_str(), p));
    }
    docs
  }
}

impl Documentation {
  #[must_use]
  pub fn from_raw(input: &str) -> Self {
    let formatted = Self::process_doc_text(input);
    Self {
      lines: formatted.lines().map(String::from).collect(),
    }
  }

  #[must_use]
  pub fn from_optional(desc: Option<&String>) -> Self {
    desc.map_or_else(Self::default, |d| Self::from_raw(d))
  }

  #[must_use]
  pub fn from_lines(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
    Self {
      lines: lines.into_iter().map(Into::into).collect(),
    }
  }

  #[must_use]
  pub fn is_empty(&self) -> bool {
    self.lines.is_empty()
  }

  #[must_use]
  pub fn lines(&self) -> &[String] {
    &self.lines
  }

  pub fn push(&mut self, line: impl Into<String>) {
    self.lines.push(line.into());
  }

  pub fn extend(&mut self, lines: impl IntoIterator<Item = impl Into<String>>) {
    self.lines.extend(lines.into_iter().map(Into::into));
  }

  pub fn clear(&mut self) {
    self.lines.clear();
  }

  #[cfg(feature = "mdformat")]
  fn process_doc_text(input: &str) -> String {
    Self::wrap_format_with_mdformat(input).replace("\\n", "\n")
  }

  #[cfg(not(feature = "mdformat"))]
  fn process_doc_text(input: &str) -> String {
    input.replace("\\n", "\n")
  }

  #[cfg(feature = "mdformat")]
  fn wrap_format_with_mdformat(input: &str) -> String {
    tokio::task::block_in_place(|| Handle::current().block_on(Self::build_async_format_with_mdformat(input)))
  }

  #[cfg(feature = "mdformat")]
  pub(crate) async fn build_async_format_with_mdformat(input: &str) -> String {
    if input.len() > 100 {
      Self::format_with_mdformat(input).await.unwrap_or_default()
    } else {
      input.to_string()
    }
  }

  #[cfg(feature = "mdformat")]
  async fn format_with_mdformat(input: &str) -> anyhow::Result<String> {
    use tokio::io::AsyncWriteExt;

    let mut child = Command::new("mdformat")
      .args(["--wrap", "100"])
      .args(["--end-of-line", "lf"])
      .arg("-")
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()?;

    {
      let mut stdin = child.stdin.take().unwrap();
      stdin.write_all(input.as_bytes()).await?;
      drop(stdin);
    };

    let output = child.wait_with_output().await?;
    if !output.status.success() {
      let stderr = String::from_utf8(output.stderr)?;
      anyhow::bail!("mdformat failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout)
  }
}

impl ToTokens for Documentation {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if self.lines.is_empty() {
      return;
    }
    let doc_lines: Vec<TokenStream> = self.lines.iter().map(|line| quote! { #[doc = #line] }).collect();
    quote! { #(#doc_lines)* }.to_tokens(tokens);
  }
}

impl From<&str> for Documentation {
  fn from(s: &str) -> Self {
    Self::from_raw(s)
  }
}

impl From<String> for Documentation {
  fn from(s: String) -> Self {
    Self::from_raw(&s)
  }
}

impl From<Option<&String>> for Documentation {
  fn from(s: Option<&String>) -> Self {
    Self::from_optional(s)
  }
}

impl<S: Into<String>> FromIterator<S> for Documentation {
  fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
    Self::from_lines(iter)
  }
}

impl From<Vec<String>> for Documentation {
  fn from(lines: Vec<String>) -> Self {
    Self { lines }
  }
}

impl PartialEq<Vec<String>> for Documentation {
  fn eq(&self, other: &Vec<String>) -> bool {
    self.lines == *other
  }
}

impl std::fmt::Display for Documentation {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    for line in &self.lines {
      writeln!(f, "{line}")?;
    }
    Ok(())
  }
}
