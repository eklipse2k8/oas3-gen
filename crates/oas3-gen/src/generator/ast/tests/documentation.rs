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
  let doc = Documentation::from_lines(["Corgi sploot documentation"]);
  let tokens = quote! { #doc };
  let expected = quote! { #[doc = " Corgi sploot documentation"] };
  assert_eq!(tokens.to_string(), expected.to_string());
}

#[test]
fn multi_line_documentation() {
  let doc = Documentation::from_lines(["Floof 1", "Floof 2"]);
  let tokens = quote! { #doc };
  let expected = quote! {
    #[doc = " Floof 1"]
    #[doc = " Floof 2"]
  };
  assert_eq!(tokens.to_string(), expected.to_string());
}

#[test]
fn from_optional_none_produces_empty() {
  let doc = Documentation::from_optional(None);
  let tokens = quote! { #doc };
  assert!(tokens.is_empty());
}

#[test]
fn from_optional_some_processes_text() {
  let text = "Bark".to_string();
  let doc = Documentation::from_optional(Some(&text));
  let expected = quote! { #[doc = " Bark"] };
  assert_eq!(quote! { #doc }.to_string(), expected.to_string());
}

#[test]
fn from_optional_handles_escaped_newlines() {
  let text = "Floof 1\\nFloof 2".to_string();
  let doc = Documentation::from_optional(Some(&text));
  let expected = quote! {
    #[doc = " Floof 1"]
    #[doc = " Floof 2"]
  };
  assert_eq!(quote! { #doc }.to_string(), expected.to_string());
}

#[test]
fn push_adds_line() {
  let mut doc = Documentation::from_lines(["Loaf"]);
  doc.push("Sploot");
  let expected = quote! {
    #[doc = " Loaf"]
    #[doc = " Sploot"]
  };
  assert_eq!(quote! { #doc }.to_string(), expected.to_string());
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
