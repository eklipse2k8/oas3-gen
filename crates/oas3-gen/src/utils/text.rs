#[cfg(feature = "mdformat")]
use std::process::Stdio;

#[cfg(feature = "mdformat")]
use tokio::{process::Command, runtime::Handle};

#[cfg(feature = "mdformat")]
#[inline]
#[must_use]
async fn build_async_format_with_mdformat(input: &str) -> String {
  if input.len() > 100 {
    format_with_mdformat(input).await.unwrap_or_default()
  } else {
    input.to_string()
  }
}

#[cfg(feature = "mdformat")]
#[inline]
#[must_use]
fn wrap_format_with_mdformat(input: &str) -> String {
  tokio::task::block_in_place(|| Handle::current().block_on(async { build_async_format_with_mdformat(input).await }))
}

#[inline]
#[must_use]
fn process_doc_text(input: &str) -> String {
  let formatted = {
    cfg_if! {
      if #[cfg(feature = "mdformat")] {
        wrap_format_with_mdformat(input)
      } else {
        input.to_string()
      }
    }
  };
  formatted.replace("\\n", "\n")
}

#[inline]
#[must_use]
pub(crate) fn doc_lines(input: &str) -> Vec<String> {
  process_doc_text(input).lines().map(String::from).collect()
}

#[inline]
#[must_use]
pub(crate) fn doc_comment_lines(input: &str) -> Vec<String> {
  process_doc_text(input)
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

#[cfg(test)]
mod tests {
  #[cfg(feature = "mdformat")]
  use super::{build_async_format_with_mdformat, split_lines};

  #[cfg(feature = "mdformat")]
  #[tokio::test]
  async fn test_doc_comment_lines_with_mdformat() {
    let input = r"## Blockquotes

> Markdown is a lightweight markup language with plain-text-formatting syntax, created in 2004 by John Gruber with Aaron Swartz.
>
>> Markdown is often used to format readme files, for writing messages in online discussion forums, and to create rich text using a plain text editor.
";
    let expected = vec![
      "/// ## Blockquotes".to_string(),
      "/// ".to_string(),
      "/// > Markdown is a lightweight markup language with plain-text-formatting syntax, created in 2004 by"
        .to_string(),
      "/// > John Gruber with Aaron Swartz.".to_string(),
      "/// >".to_string(),
      "/// > > Markdown is often used to format readme files, for writing messages in online discussion forums,"
        .to_string(),
      "/// > > and to create rich text using a plain text editor.".to_string(),
    ];
    let result = split_lines(&build_async_format_with_mdformat(input).await);
    assert_eq!(result, expected);
  }
}
