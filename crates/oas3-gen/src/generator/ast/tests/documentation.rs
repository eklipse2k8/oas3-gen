use quote::quote;

use crate::generator::ast::Documentation;

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
