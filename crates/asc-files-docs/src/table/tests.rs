// SPDX-License-Identifier: MIT

use super::*;

// Legacy's table tests (`services/table/tests.rs`), checking the HTML.
#[test]
fn delimited_tables_preserve_quotes_newlines_ragged_rows_and_literal_markup() {
    let table = parse_delimited(
        "name,value\n\"a,b\",\"line 1\nline 2\"\n<script>,2,extra\n",
        b',',
    )
    .expect("quoted CSV");
    assert_eq!(table.rows[1], ["a,b", "line 1\nline 2"]);
    assert_eq!(table.rows[2], ["<script>", "2", "extra"]);
    let html = table.to_html();
    assert!(
        html.starts_with("<table><tr><th>name</th><th>value</th></tr>"),
        "{html}"
    );
    assert!(html.contains("<td>&lt;script&gt;</td>"), "{html}");
    let table = parse_delimited("a\tb\n1\t2\n", b'\t').expect("TSV");
    assert_eq!(table.rows.len(), 2);
}

#[test]
fn tables_are_not_limited_to_200_rows_or_512_cells() {
    let source = format!("a,b,c\n{}", "1,2,3\n".repeat(1000));
    let table = parse_delimited(&source, b',').expect("large table");
    assert_eq!(table.rows.len(), 1001);
    assert!(!table.truncated);
}

#[test]
fn the_column_budget_is_observable() {
    let source = vec!["cell"; TABLE_COLUMN_LIMIT + 1].join(",");
    let table = parse_delimited(&source, b',').expect("bounded wide table");
    assert!(table.truncated);
    assert_eq!(table.rows[0].len(), TABLE_COLUMN_LIMIT);
}

#[test]
fn reads_first_worksheet_in_each_supported_workbook_format() {
    for extension in ["xls", "xlsx", "ods"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "tests/fixtures/spreadsheets/any_sheets.{extension}"
        ));
        let table = read_workbook(&path).expect("workbook fixture");
        assert_eq!(table.rows[0], ["1", "2"], "{extension}");
        assert_eq!(table.rows[1], ["3", "4"], "{extension}");
        assert_eq!(table.rows[2], ["5", "6"], "{extension}");
        assert_eq!(table.rows[3], ["", ""], "{extension}");
        assert!(table.rows[4][0].contains("4 sheets"), "{extension}");
        assert_eq!(table.rows.len(), 5, "only the first worksheet is previewed");
        assert!(!table.truncated);
    }
}

#[test]
fn rejects_oversized_and_invalid_workbooks() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let path = directory.path().join("book.xlsx");
    std::fs::write(&path, "not a workbook").expect("invalid fixture");
    assert!(read_workbook(&path).is_err());
    std::fs::File::create(&path)
        .expect("sparse fixture")
        .set_len(WORKBOOK_BYTE_LIMIT + 1)
        .expect("oversized fixture");
    assert!(
        read_workbook(&path)
            .expect_err("oversized workbook")
            .contains("too large")
    );
}
