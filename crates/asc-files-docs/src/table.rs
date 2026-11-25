// SPDX-License-Identifier: MIT

use std::path::Path;

use calamine::Reader;

pub(crate) const WORKBOOK_BYTE_LIMIT: u64 = 20 * 1024 * 1024;
const TABLE_TEXT_LIMIT: usize = 4 * 1024 * 1024;
const TABLE_VALUE_LIMIT: usize = 100_000;
pub(crate) const TABLE_COLUMN_LIMIT: usize = 256;

#[derive(Debug, Default)]
pub(crate) struct TableData {
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

impl TableData {
    /// The table as HTML: the first row as headings.
    pub(crate) fn to_html(&self) -> String {
        let mut html = String::from("<table>");
        for (index, row) in self.rows.iter().enumerate() {
            html.push_str("<tr>");
            let cell = if index == 0 { "th" } else { "td" };
            for text in row {
                html.push_str(&format!(
                    "<{cell}>{}</{cell}>",
                    crate::escape(&text.replace('\0', "\u{fffd}"))
                ));
            }
            html.push_str("</tr>");
        }
        html.push_str("</table>");
        html
    }
}

pub(crate) fn parse_delimited(source: &str, delimiter: u8) -> Result<TableData, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(source.as_bytes());
    let mut table = TableData {
        rows: Vec::new(),
        truncated: false,
    };
    let mut values = 0;
    let mut text = 0;
    for record in reader.records() {
        let record = record.map_err(|e| format!("Unable to parse table: {e}"))?;
        if !push_row(
            &mut table,
            record.iter().map(str::to_owned),
            &mut values,
            &mut text,
        ) {
            break;
        }
    }
    if table.rows.is_empty() {
        return Err("The table has no rows".into());
    }
    Ok(table)
}

fn push_row(
    table: &mut TableData,
    row: impl Iterator<Item = String>,
    values: &mut usize,
    text: &mut usize,
) -> bool {
    let mut cells = Vec::new();
    for cell in row {
        if cells.len() >= TABLE_COLUMN_LIMIT
            || *values >= TABLE_VALUE_LIMIT
            || text.saturating_add(cell.len()) > TABLE_TEXT_LIMIT
        {
            table.truncated = true;
            break;
        }
        *values += 1;
        *text += cell.len();
        cells.push(cell);
    }
    if cells.is_empty() {
        *values += 1;
    }
    if *values > TABLE_VALUE_LIMIT {
        table.truncated = true;
        return false;
    }
    table.rows.push(cells);
    !table.truncated
}

// Called only by the resource-limited sandbox helper: obtaining a calamine range
// may decompress and allocate the entire worksheet before output budgets apply.
pub(crate) fn read_workbook(path: &Path) -> Result<TableData, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > WORKBOOK_BYTE_LIMIT {
        return Err("Workbook is too large to preview safely".into());
    }
    let mut workbook = calamine::open_workbook_auto(path).map_err(|e| e.to_string())?;
    let range = workbook
        .worksheet_range_at(0)
        .ok_or("Workbook has no worksheets")?
        .map_err(|e| e.to_string())?;
    let mut table = TableData {
        rows: Vec::new(),
        truncated: false,
    };
    let mut values = 0;
    let mut text = 0;
    for row in range.rows() {
        if !push_row(
            &mut table,
            row.iter().map(ToString::to_string),
            &mut values,
            &mut text,
        ) {
            break;
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests;
