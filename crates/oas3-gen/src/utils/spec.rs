use std::{ffi::OsStr, path::Path};

use fmmap::tokio::{AsyncMmapFile, AsyncMmapFileExt};
use oas3::OpenApiV3Spec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpecFormat {
  #[default]
  Json,
  Yaml,
}

impl SpecFormat {
  #[must_use]
  pub fn from_extension(ext: &str) -> Self {
    match ext {
      "yaml" | "yml" => Self::Yaml,
      _ => Self::Json,
    }
  }
}

pub struct SpecLoader {
  file: AsyncMmapFile,
  format: SpecFormat,
}

impl SpecLoader {
  pub async fn open(path: &Path) -> anyhow::Result<Self> {
    let format = path
      .extension()
      .and_then(OsStr::to_str)
      .map_or(SpecFormat::default(), SpecFormat::from_extension);

    let file = AsyncMmapFile::open(path).await?;

    Ok(Self { file, format })
  }

  pub fn parse(&self) -> anyhow::Result<oas3::Spec> {
    match self.format {
      SpecFormat::Json => Ok(serde_json::from_slice::<OpenApiV3Spec>(self.file.as_slice())?),
      SpecFormat::Yaml => {
        let content = std::str::from_utf8(self.file.as_slice())?;
        Ok(oas3::from_yaml(content)?)
      }
    }
  }
}
