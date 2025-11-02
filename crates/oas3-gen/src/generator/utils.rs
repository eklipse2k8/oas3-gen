#[cfg(feature = "mdformat")]
use std::process::Stdio;

#[cfg(feature = "mdformat")]
use tokio::process::Command;

/// Convert a string into doc comment lines with `///` prefix
pub(crate) async fn doc_comment_lines(input: &str) -> Vec<String> {
  #[cfg(feature = "mdformat")]
  let reformatted = format_with_mdformat(input).await.unwrap_or_default();
  #[cfg(not(feature = "mdformat"))]
  let reformatted = input.to_string();

  let normalized = reformatted.replace("\\n", "\n");
  normalized
    .lines()
    .map(|line| {
      if line.is_empty() {
        "/// ".to_string()
      } else {
        format!("/// {line}")
      }
    })
    .collect()
}

#[cfg(feature = "mdformat")]
async fn format_with_mdformat(input: &str) -> anyhow::Result<String> {
  use tokio::io::AsyncWriteExt;

  let mut child = Command::new("uvx")
    .arg("mdformat")
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

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(feature = "mdformat")]
  #[tokio::test]
  async fn test_doc_comment_lines() {
    let input = "This is a test.\n\nNew line here.\nLine with \n escaped newline.";
    let expected = vec![
      "/// This is a test.".to_string(),
      "/// ".to_string(),
      "/// New line here. Line with escaped newline.".to_string(),
    ];
    let result = doc_comment_lines(input).await;
    assert_eq!(result, expected);
  }
}
