//! RevuCraft: spreadsheet tables (XLSX, XLSM, XLS, ODS, CSV) drawn as PDF content, through
//! GridCraft's reader, calculation engine and printer (Azurehawke/gridcraft, MIT OR Apache-2.0).
//!
//! A workbook is opened and every formula worked out again, so a table shows what the workbook
//! says now (also for a CSV, which has no saved results). A range of one sheet (its print area or
//! used range when none is named) is printed by GridCraft onto one page exactly its size: no
//! margins, header or footer, at the sheet's own column widths, row heights, fonts, fills, borders
//! and number formats. RevuCraft places that page on a sheet as a form, as it places drawings.

use gridcraft_engine::calc::Calc;
use gridcraft_engine::cmd::print::{PrintOptions, layout_sheet, render_pdf};
use gridcraft_engine::core::{CellRef, RangeRef, col_to_letters};
use gridcraft_engine::model::Workbook;

use crate::CreateError;

/// The files a table can come from, by extension.
pub const EXTENSIONS: [&str; 6] = ["xlsx", "xlsm", "xls", "ods", "csv", "tsv"];

/// Workbooks larger than this aren't read (a table is a schedule, not a database).
pub const MAX_BYTES: usize = 64 << 20;
/// The most rows and columns a placed table shows; a larger range is cut and says so.
pub const MAX_ROWS: u32 = 400;
pub const MAX_COLS: u32 = 60;
/// The most rows and columns the workbook view shows of a sheet.
pub const VIEW_ROWS: u32 = 500;
pub const VIEW_COLS: u32 = 52;
/// GridCraft measures columns and rows in pixels at 96 per inch.
const PX_TO_PT: f64 = 0.75;

/// Whether `name` is a spreadsheet file a table can come from.
pub fn is_workbook(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    EXTENSIONS.contains(&ext.as_str())
}

fn invalid(msg: impl Into<String>) -> CreateError {
    CreateError::Invalid(msg.into())
}

/// An opened workbook, its formulas worked out.
pub struct Book {
    wb: Workbook,
}

/// One sheet as text, for the workbook view: the cells of its used range (capped at
/// [`VIEW_ROWS`] × [`VIEW_COLS`]) as they show, and the column widths in points.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SheetGrid {
    pub name: String,
    /// The first row and column shown (0-based).
    pub origin: (u32, u32),
    pub widths: Vec<f32>,
    pub rows: Vec<Vec<String>>,
    /// The used range was larger than what is shown.
    pub cut: bool,
}

/// A column's letters (0 → `A`, 26 → `AA`), for headings over a [`SheetGrid`].
pub fn column_letters(col: u32) -> String {
    col_to_letters(col)
}

/// A table printed as a one-page PDF, `size` points wide and high.
#[derive(Clone, Debug)]
pub struct TablePlot {
    pub pdf: Vec<u8>,
    pub size: [f64; 2],
    /// The range shown (`A1:F20`).
    pub range: String,
    pub notes: Vec<String>,
}

/// Open the workbook `name` (`bytes`) and work out its formulas.
pub fn open(bytes: &[u8], name: &str) -> Result<Book, CreateError> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid(format!("{name} is too large to show as a table")));
    }
    let (mut wb, _) = gridcraft_engine::io::open_bytes(name, bytes).map_err(|e| invalid(format!("{name}: {e}")))?;
    let mut calc = Calc::new();
    calc.recalc_all(&mut wb);
    Ok(Book { wb })
}

impl Book {
    /// The sheets' names, in tab order.
    pub fn sheet_names(&self) -> Vec<String> {
        self.wb.sheets.iter().map(|s| s.name.clone()).collect()
    }

    /// The workbook's named ranges (`Budget`, `Totals`), for choosing a table by name.
    pub fn range_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.wb.names.iter().filter(|n| !n.hidden && named_range(&self.wb, &n.formula).is_some()).map(|n| n.name.clone()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The sheet and range `range` names on sheet `sheet`: `B2:F30`, a named range, or empty for
    /// the sheet's print area (else its used range).
    pub fn resolve(&self, sheet: &str, range: &str) -> Result<(usize, RangeRef), CreateError> {
        let range = range.trim();
        if !range.is_empty()
            && let Some(n) = self.wb.names.iter().find(|n| n.name.eq_ignore_ascii_case(range))
        {
            return named_range(&self.wb, &n.formula).ok_or_else(|| invalid(format!("{range} isn't a range of cells")));
        }
        let si = self
            .wb
            .sheet_index(sheet)
            .or_else(|| (sheet.is_empty() && !self.wb.sheets.is_empty()).then_some(0))
            .ok_or_else(|| invalid(format!("the workbook has no sheet called {sheet}")))?;
        let sh = self.wb.sheet(si).ok_or_else(|| invalid("that sheet can't be read"))?;
        if range.is_empty() {
            let r = sh.print.print_area.or_else(|| sh.used_range()).ok_or_else(|| invalid(format!("{} is empty", sh.name)))?;
            return Ok((si, r));
        }
        let (_, cells) = range.rsplit_once('!').unwrap_or(("", range));
        let r = RangeRef::parse(&cells.replace('$', "")).ok_or_else(|| invalid(format!("{range} isn't a range like B2:F30")))?;
        Ok((si, r))
    }

    /// Sheet `index` as text, for the workbook view.
    pub fn grid(&self, index: usize) -> Option<SheetGrid> {
        let sh = self.wb.sheet(index)?;
        let Some(used) = sh.used_range() else {
            return Some(SheetGrid { name: sh.name.clone(), ..SheetGrid::default() });
        };
        let last_row = used.end.row.min(used.start.row.saturating_add(VIEW_ROWS - 1));
        let last_col = used.end.col.min(used.start.col.saturating_add(VIEW_COLS - 1));
        let cols: Vec<u32> = (used.start.col..=last_col).filter(|c| !sh.is_col_hidden(*c)).collect();
        let widths = cols.iter().map(|c| (f64::from(sh.col_width(*c)) * PX_TO_PT) as f32).collect();
        let rows = (used.start.row..=last_row)
            .filter(|r| !sh.is_row_hidden(*r))
            .map(|r| cols.iter().map(|c| gridcraft_engine::display::value_text(&self.wb, sh, CellRef::new(r, *c))).collect())
            .collect();
        Some(SheetGrid {
            name: sh.name.clone(),
            origin: (used.start.row, used.start.col),
            widths,
            rows,
            cut: last_row < used.end.row || last_col < used.end.col,
        })
    }

    /// The range `range` of sheet `sheet` printed onto one page its own size (see [`plot_table`]).
    pub fn plot(&self, sheet: &str, range: &str, file: &str) -> Result<TablePlot, CreateError> {
        let (si, r) = self.resolve(sheet, range)?;
        let sh = self.wb.sheet(si).ok_or_else(|| invalid("that sheet can't be read"))?;
        let mut notes = Vec::new();
        let rows = r.end.row.saturating_sub(r.start.row).saturating_add(1);
        let cols = r.end.col.saturating_sub(r.start.col).saturating_add(1);
        let area = if rows > MAX_ROWS || cols > MAX_COLS {
            notes.push(format!("only the first {MAX_ROWS} rows and {MAX_COLS} columns are shown"));
            RangeRef::new(
                r.start,
                CellRef::new(r.start.row.saturating_add(rows.min(MAX_ROWS) - 1), r.start.col.saturating_add(cols.min(MAX_COLS) - 1)),
            )
        } else {
            r
        };
        let opts = PrintOptions { range: Some(area), ..PrintOptions::default() };
        let mut lay = layout_sheet(&self.wb, si, &opts, Some(area)).ok_or_else(|| invalid("that range can't be laid out"))?;
        let area = lay.area;
        let w: f64 = (area.start.col..=area.end.col).map(|c| f64::from(sh.col_width(c))).sum::<f64>() * PX_TO_PT;
        let h: f64 = (area.start.row..=area.end.row).map(|r| f64::from(sh.row_height(r))).sum::<f64>() * PX_TO_PT;
        if !(w.is_finite() && h.is_finite()) || w < 1.0 || h < 1.0 {
            return Err(invalid("that range has no visible cells"));
        }
        // One page, exactly the range: no margins, header, footer, headings or repeated titles.
        lay.scale = 1.0;
        lay.margins = [0.0; 6];
        lay.page_w = w as f32;
        lay.page_h = h as f32;
        lay.heading_w = 0.0;
        lay.heading_h = 0.0;
        lay.title_rows = None;
        lay.title_cols = None;
        lay.row_bands = vec![(area.start.row, area.end.row)];
        lay.col_bands = vec![(area.start.col, area.end.col)];
        lay.pages = vec![(0, 0)];
        lay.settings.headings = false;
        lay.settings.center_h = false;
        lay.settings.center_v = false;
        lay.settings.header.clear();
        lay.settings.footer.clear();
        lay.settings.different_first = false;
        lay.settings.different_odd_even = false;
        let (pdf, pages) = render_pdf(&self.wb, &[lay], file, "");
        if pages == 0 {
            return Err(invalid("that range couldn't be printed"));
        }
        Ok(TablePlot { pdf, size: [w, h], range: a1(area), notes })
    }
}

/// The sheet and cells a named range's formula (`Budget!$A$1:$F$20`) refers to.
fn named_range(wb: &Workbook, formula: &str) -> Option<(usize, RangeRef)> {
    let (sheet, cells) = formula.trim().trim_start_matches('=').rsplit_once('!')?;
    let sheet = sheet.trim_matches('\'').replace("''", "'");
    let si = wb.sheet_index(&sheet)?;
    Some((si, RangeRef::parse(&cells.replace('$', ""))?))
}

/// A range in A1 notation.
fn a1(r: RangeRef) -> String {
    let cell = |c: CellRef| format!("{}{}", col_to_letters(c.col), u64::from(c.row) + 1);
    format!("{}:{}", cell(r.start), cell(r.end))
}

/// Range `range` of sheet `sheet` of workbook `name` (`bytes`), printed onto one page its own
/// size. An empty `sheet` is the first sheet; an empty `range` the sheet's print area or used
/// range.
pub fn plot_table(bytes: &[u8], name: &str, sheet: &str, range: &str) -> Result<TablePlot, CreateError> {
    open(bytes, name)?.plot(sheet, range, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "Item,Qty,Unit cost\nData drop,24,85\nAccess point,6,410\n";

    /// A budget with formulas, written by GridCraft itself.
    fn budget_xlsx() -> Vec<u8> {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        for (cell, input) in [
            ("A1", "Item"),
            ("B1", "Qty"),
            ("C1", "Unit cost"),
            ("D1", "Total"),
            ("A2", "Data drop"),
            ("B2", "24"),
            ("C2", "85"),
            ("D2", "=B2*C2"),
            ("A3", "Access point"),
            ("B3", "6"),
            ("C3", "410"),
            ("D3", "=B3*C3"),
            ("A4", "Total"),
            ("D4", "=SUM(D2:D3)"),
        ] {
            s.execute("cell.set", serde_json::json!({ "cell": cell, "input": input })).unwrap();
        }
        gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "budget.xlsx").unwrap()
    }

    #[test]
    fn a_budget_prints_as_a_page_its_own_size_with_its_formulas_worked_out() {
        let book = open(&budget_xlsx(), "budget.xlsx").unwrap();
        let grid = book.grid(0).unwrap();
        assert_eq!(grid.rows.len(), 4);
        assert_eq!(grid.rows[3][3], "4500", "24 × 85 + 6 × 410");
        let plot = book.plot("", "", "budget.xlsx").unwrap();
        assert_eq!(plot.range, "A1:D4");
        assert!(plot.size[0] > 100.0 && plot.size[1] > 20.0, "{:?}", plot.size);
        let doc = pdfcraft_cos::Document::open(std::sync::Arc::new(plot.pdf)).unwrap();
        assert_eq!(pdfcraft_model::pages(&doc).len(), 1);
        let only = book.plot("", "A1:B2", "budget.xlsx").unwrap();
        assert_eq!(only.range, "A1:B2");
        assert!(only.size[0] < plot.size[0]);
    }

    #[test]
    fn a_csv_list_shows_as_it_reads() {
        let book = open(LIST.as_bytes(), "list.csv").unwrap();
        assert_eq!(book.sheet_names().len(), 1);
        let grid = book.grid(0).unwrap();
        assert_eq!(grid.rows.len(), 3);
        assert_eq!(grid.rows[1][1], "24");
        assert!(book.plot("", "", "list.csv").is_ok());
    }

    #[test]
    fn bad_ranges_and_files_are_refused() {
        let book = open(LIST.as_bytes(), "list.csv").unwrap();
        assert!(book.plot("", "not a range", "list.csv").is_err());
        assert!(book.plot("Nope", "", "list.csv").is_err());
        assert!(open(b"PK\x03\x04garbage", "budget.xlsx").is_err());
        assert!(is_workbook("Suite budget.XLSX") && !is_workbook("plan.dwg"));
        assert_eq!(a1(RangeRef::new(CellRef::new(0, 0), CellRef::new(9, 27))), "A1:AB10");
    }
}
