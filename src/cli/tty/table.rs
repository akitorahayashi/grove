use std::io;

use owo_colors::OwoColorize;
use unicode_width::UnicodeWidthStr;

use crate::cli::output::Output;

const COLUMN_GAP: &str = "  ";

/// Color carried by a rendered cell.
#[derive(Debug, Clone, Copy)]
pub(in crate::cli) enum Paint {
    Bold,
    Dimmed,
    Cyan,
    Blue,
    Green,
    Yellow,
    Red,
}

/// A single cell: display-ready text paired with the color it carries.
///
/// Text is expected to already be escaped for terminal display; the table only
/// measures, pads, and colors it.
#[derive(Debug)]
pub(in crate::cli) struct Cell {
    text: String,
    paint: Paint,
}

impl Cell {
    pub(in crate::cli) fn new(text: impl Into<String>, paint: Paint) -> Self {
        Self { text: text.into(), paint }
    }
}

/// A column-aligned table with a colored header and a dimmed rule.
///
/// Styling is emitted unconditionally; [`Output`] strips ANSI when the
/// destination and environment call for plain text, so the color decision stays
/// centralized rather than duplicated per call site.
#[derive(Debug)]
pub(in crate::cli) struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub(in crate::cli) fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self { headers: headers.into_iter().map(Into::into).collect(), rows: Vec::new() }
    }

    pub(in crate::cli) fn push_row(&mut self, cells: Vec<Cell>) {
        debug_assert_eq!(
            cells.len(),
            self.headers.len(),
            "table row must have one cell per header column"
        );
        self.rows.push(cells);
    }

    pub(in crate::cli) fn render(&self, output: &mut Output<'_>) -> io::Result<()> {
        let widths = self.column_widths();

        output.stdout(format_args!("\n"))?;
        self.render_header(&widths, output)?;
        self.render_rule(&widths, output)?;
        for row in &self.rows {
            let cells = row
                .iter()
                .zip(&widths)
                .map(|(cell, &width)| apply(pad(&cell.text, width), cell.paint));
            write_row(cells, output)?;
        }
        Ok(())
    }

    fn column_widths(&self) -> Vec<usize> {
        let mut widths: Vec<usize> = self.headers.iter().map(|header| header.width()).collect();
        for row in &self.rows {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.text.width());
            }
        }
        widths
    }

    fn render_header(&self, widths: &[usize], output: &mut Output<'_>) -> io::Result<()> {
        let cells = self
            .headers
            .iter()
            .zip(widths)
            .map(|(header, &width)| pad(header, width).yellow().bold().to_string());
        write_row(cells, output)
    }

    fn render_rule(&self, widths: &[usize], output: &mut Output<'_>) -> io::Result<()> {
        let span =
            widths.iter().sum::<usize>() + COLUMN_GAP.len() * self.headers.len().saturating_sub(1);
        output.stdout(format_args!("{}\n", "-".repeat(span).dimmed()))
    }
}

fn write_row(cells: impl Iterator<Item = String>, output: &mut Output<'_>) -> io::Result<()> {
    let line = cells.collect::<Vec<_>>().join(COLUMN_GAP);
    output.stdout(format_args!("{line}\n"))
}

/// Right-pads `text` with spaces to reach `width` terminal columns.
///
/// Padding is based on display width, not scalar count, so wide (CJK) and
/// zero-width (combining) characters keep following columns aligned.
fn pad(text: &str, width: usize) -> String {
    let deficit = width.saturating_sub(text.width());
    let mut padded = String::with_capacity(text.len() + deficit);
    padded.push_str(text);
    padded.push_str(&" ".repeat(deficit));
    padded
}

fn apply(text: String, paint: Paint) -> String {
    match paint {
        Paint::Bold => text.bold().to_string(),
        Paint::Dimmed => text.dimmed().to_string(),
        Paint::Cyan => text.cyan().to_string(),
        Paint::Blue => text.blue().to_string(),
        Paint::Green => text.green().to_string(),
        Paint::Yellow => text.yellow().to_string(),
        Paint::Red => text.red().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::{Cell, Paint, Table, pad};

    #[test]
    fn pads_wide_characters_by_display_width() {
        // "東京" is two scalars but four terminal columns.
        let padded = pad("東京", 6);
        assert_eq!(padded, "東京  ");
        assert_eq!(padded.width(), 6);
    }

    #[test]
    fn pads_combining_sequence_by_display_width() {
        // "e" + combining acute renders in one column despite two scalars.
        let padded = pad("e\u{0301}", 3);
        assert!(padded.starts_with("e\u{0301}"));
        assert_eq!(padded.width(), 3);
    }

    #[test]
    fn does_not_truncate_when_content_exceeds_width() {
        assert_eq!(pad("東京", 2), "東京");
    }

    #[test]
    fn column_width_uses_display_width_not_scalar_count() {
        let mut table = Table::new(["A"]);
        table.push_row(vec![Cell::new("東", Paint::Bold)]);

        assert_eq!(table.column_widths(), vec![2]);
    }
}
