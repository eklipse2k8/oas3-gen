/// Convert a string into doc comment lines with `///` prefix
pub(crate) fn doc_comment_lines(input: &str) -> Vec<String> {
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
