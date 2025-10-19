/// Convert a string into doc comment lines with `///` prefix
pub fn doc_comment_lines(input: &str) -> Vec<String> {
  let normalized = input.replace("\\n", "\n");
  normalized
    .lines()
    .map(|line| {
      if line.is_empty() {
        "/// ".to_string()
      } else {
        format!("/// {}", line)
      }
    })
    .collect()
}

/// Convert a string into a doc comment block
pub fn doc_comment_block(input: &str) -> String {
  doc_comment_lines(input).join("\n")
}
