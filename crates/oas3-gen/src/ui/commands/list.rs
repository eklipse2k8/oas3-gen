use std::path::PathBuf;

use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Row, Table};

use crate::{
  reserved::to_rust_field_name,
  ui::{Colors, colors::IntoComfyColor, term_width},
};

fn generate_operation_id(method: &str, path: &str) -> String {
  let path_parts: Vec<&str> = path
    .split('/')
    .filter(|s| !s.is_empty())
    .map(|s| {
      if s.starts_with('{') && s.ends_with('}') {
        "by_id"
      } else {
        s
      }
    })
    .collect();

  let method_lower = method.to_lowercase();
  if path_parts.is_empty() {
    method_lower
  } else {
    format!("{}_{}", method_lower, path_parts.join("_"))
  }
}

pub async fn list_operations(input: &PathBuf, colors: &Colors) -> anyhow::Result<()> {
  let file_content = tokio::fs::read_to_string(input).await?;
  let spec: oas3::Spec = oas3::from_json(file_content)?;

  let mut operations = Vec::new();

  for (path, method, operation) in spec.operations() {
    let id = operation
      .operation_id
      .clone()
      .unwrap_or_else(|| generate_operation_id(method.as_str(), &path));

    let id = to_rust_field_name(&id);
    operations.push((id, method.as_str().to_string(), path));
  }

  operations.sort_by(|a, b| a.0.cmp(&b.0));

  let mut table = Table::new();
  table
    .load_preset("  ── ──            ")
    .set_content_arrangement(ContentArrangement::Dynamic)
    .set_width(term_width());

  let mut row = Row::new();
  row.add_cell(Cell::new("OPERATION ID").fg(IntoComfyColor::into(colors.label())));
  row.add_cell(Cell::new("METHOD").fg(IntoComfyColor::into(colors.label())));
  row.add_cell(Cell::new("PATH").fg(IntoComfyColor::into(colors.label())));
  table.set_header(row);

  for (operation_id, method, path) in operations {
    let mut row = Row::new();
    row.add_cell(
      Cell::new(operation_id)
        .fg(IntoComfyColor::into(colors.value()))
        .add_attribute(Attribute::Bold),
    );
    row.add_cell(
      Cell::new(method)
        .fg(IntoComfyColor::into(colors.accent()))
        .set_alignment(CellAlignment::Right),
    );
    row.add_cell(Cell::new(path).fg(IntoComfyColor::into(colors.primary())));
    table.add_row(row);
  }

  println!("{table}");

  Ok(())
}
