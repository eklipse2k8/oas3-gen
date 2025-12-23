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

#[allow(dead_code)]
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
  async fn build_async_format_with_mdformat(input: &str) -> String {
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

#[cfg(test)]
mod tests {
  use quote::quote;

  use super::*;

  #[test]
  fn empty_documentation_produces_no_tokens() {
    let doc = Documentation::default();
    let tokens = quote! { #doc };
    assert!(tokens.is_empty());
  }

  #[test]
  fn single_line_documentation() {
    let doc = Documentation::from_lines(["Test documentation"]);
    let tokens = quote! { #doc };
    let expected = quote! { #[doc = "Test documentation"] };
    assert_eq!(tokens.to_string(), expected.to_string());
  }

  #[test]
  fn multi_line_documentation() {
    let doc = Documentation::from_lines(["Line 1", "Line 2"]);
    let tokens = quote! { #doc };
    let expected = quote! {
      #[doc = "Line 1"]
      #[doc = "Line 2"]
    };
    assert_eq!(tokens.to_string(), expected.to_string());
  }

  #[test]
  fn from_raw_handles_newlines() {
    let doc = Documentation::from_raw("Line 1\nLine 2");
    assert_eq!(doc.lines(), &["Line 1", "Line 2"]);
  }

  #[test]
  fn from_raw_handles_escaped_newlines() {
    let doc = Documentation::from_raw("Line 1\\nLine 2");
    assert_eq!(doc.lines(), &["Line 1", "Line 2"]);
  }

  #[test]
  fn from_optional_none_produces_empty() {
    let doc = Documentation::from_optional(None);
    assert!(doc.is_empty());
  }

  #[test]
  fn from_optional_some_processes_text() {
    let text = "Test".to_string();
    let doc = Documentation::from_optional(Some(&text));
    assert_eq!(doc.lines(), &["Test"]);
  }

  #[test]
  fn push_adds_line() {
    let mut doc = Documentation::from_lines(["First"]);
    doc.push("Second");
    assert_eq!(doc.lines(), &["First", "Second"]);
  }

  #[test]
  fn extend_adds_lines() {
    let mut doc = Documentation::from_lines(["First"]);
    doc.extend(["Second", "Third"]);
    assert_eq!(doc.lines(), &["First", "Second", "Third"]);
  }

  #[cfg(feature = "mdformat")]
  #[tokio::test]
  async fn test_doc_lines_with_mdformat() {
    let input = r"## Blockquotes

> Markdown is a lightweight markup language with plain-text-formatting syntax, created in 2004 by John Gruber with Aaron Swartz.
>
>> Markdown is often used to format readme files, for writing messages in online discussion forums, and to create rich text using a plain text editor.
";
    let expected = vec![
      "## Blockquotes",
      "",
      "> Markdown is a lightweight markup language with plain-text-formatting syntax, created in 2004 by",
      "> John Gruber with Aaron Swartz.",
      ">",
      "> > Markdown is often used to format readme files, for writing messages in online discussion forums,",
      "> > and to create rich text using a plain text editor.",
    ];
    let result = Documentation::build_async_format_with_mdformat(input).await;
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines, expected);
  }
}
