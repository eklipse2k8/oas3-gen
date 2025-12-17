use std::path::Path;

use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Row, Table};

use crate::{
  generator::operation_registry::OperationRegistry,
  ui::{Colors, colors::IntoComfyColor, term_width},
  utils::spec::SpecLoader,
};

pub async fn list_operations(input: &Path, colors: &Colors) -> anyhow::Result<()> {
  let spec = SpecLoader::open(input).await?.parse()?;

  let registry = OperationRegistry::from_spec(&spec);

  let mut operations: Vec<_> = registry
    .operations()
    .map(|(id, location)| (id.to_string(), location.method.clone(), location.path.clone()))
    .collect();

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
