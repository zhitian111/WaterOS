#![no_std]

#![forbid(unsafe_code)]
//! Allocation-free text table formatting over [`core::fmt`].
//!
//! The compact layout is derived from the MIT-licensed `tabled` 0.20.0 and
//! `papergrid` 0.17.0 compact renderers. See `UPSTREAM.md` in the package.

use core::fmt::{self, Debug, Display, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod fixed_table;
pub use fixed_table::{FixedTable, FixedTableWriter};
mod auto_table;
pub use auto_table::AutoTable;

/// Horizontal alignment within a column's content area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    /// Place padding after the value.
    Left,
    /// Split padding around the value.
    Center,
    /// Place padding before the value.
    Right,
}

/// Behavior when a formatted value is wider than its column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overflow {
    /// Return [`Error::ContentOverflow`] before writing the row.
    Error,
    /// Clip the value and append this marker.
    Truncate(&'static str),
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    let column = Column::new(4, Alignment::Left).padding(1, 2);
    assert_eq!(column.width(), 4);
    assert_eq!(Alignment::Right, Alignment::Right);
    assert_eq!(Overflow::Error, Overflow::Error);
}

/// A fixed-width column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Column {
    width : usize,
    alignment : Alignment,
    padding_left : usize,
    padding_right : usize,
    overflow : Overflow,
}

impl Column {
    /// Create a column. `width` counts content cells, excluding padding.
    pub const fn new(width : usize, alignment : Alignment) -> Self {
        Self { width,
               alignment,
               padding_left : 1,
               padding_right : 1,
               overflow : Overflow::Error }
    }

    /// Set left and right padding.
    pub const fn padding(mut self, left : usize, right : usize) -> Self {
        self.padding_left = left;
        self.padding_right = right;
        self
    }

    /// Set overflow behavior.
    pub const fn overflow(mut self, overflow : Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Return the content width.
    pub const fn width(&self) -> usize { self.width }
}

/// Border characters used by a table.
///
/// This is the compact border model used by `papergrid`: a frame plus a
/// horizontal separator, with one intersection character for each position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Style {
    top_left : char,
    top : char,
    top_intersection : char,
    top_right : char,
    left : char,
    vertical : char,
    right : char,
    left_intersection : char,
    horizontal : char,
    intersection : char,
    right_intersection : char,
    bottom_left : char,
    bottom : char,
    bottom_intersection : char,
    bottom_right : char,
}

impl Style {
    /// ASCII `+---+` / `| ... |` table style.
    pub const fn ascii() -> Self {
        Self { top_left : '+',
               top : '-',
               top_intersection : '+',
               top_right : '+',
               left : '|',
               vertical : '|',
               right : '|',
               left_intersection : '+',
               horizontal : '-',
               intersection : '+',
               right_intersection : '+',
               bottom_left : '+',
               bottom : '-',
               bottom_intersection : '+',
               bottom_right : '+' }
    }

    /// Unicode box-drawing style.
    pub const fn modern() -> Self {
        Self { top_left : '┌',
               top : '─',
               top_intersection : '┬',
               top_right : '┐',
               left : '│',
               vertical : '│',
               right : '│',
               left_intersection : '├',
               horizontal : '─',
               intersection : '┼',
               right_intersection : '┤',
               bottom_left : '└',
               bottom : '─',
               bottom_intersection : '┴',
               bottom_right : '┘' }
    }
}

/// A borrowed cell value.
#[derive(Clone, Copy)]
pub enum Cell<'a> {
    /// A string cell.
    Text(&'a str),
    /// A value formatted with [`Display`].
    Display(&'a dyn Display),
    /// A value formatted with [`Debug`].
    Debug(&'a dyn Debug),
}

impl<'a> Cell<'a> {
    /// Construct a string cell.
    pub const fn text(value : &'a str) -> Self { Self::Text(value) }

    /// Construct a [`Display`] cell.
    pub fn display<T : Display>(value : &'a T) -> Self { Self::Display(value) }

    /// Construct a [`Debug`] cell.
    pub fn debug<T : Debug>(value : &'a T) -> Self { Self::Debug(value) }

    fn format(&self, output : &mut dyn Write) -> fmt::Result {
        match self {
            Self::Text(value) => output.write_str(value),
            Self::Display(value) => write!(output, "{value}"),
            Self::Debug(value) => write!(output, "{value:?}"),
        }
    }
}

/// A table formatting failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The destination rejected output.
    Fmt,
    /// A row did not contain exactly one cell per column.
    ColumnCount { expected : usize, actual : usize },
    /// A zero-width column cannot represent its configured truncation marker.
    InvalidWidth { column : usize },
    /// A cell contained a newline.
    MultilineUnsupported { column : usize },
    /// A cell exceeded a column configured with [`Overflow::Error`].
    ContentOverflow {
        column : usize,
        width : usize,
        actual : usize,
    },
    /// A truncation marker is wider than its content area.
    TruncationMarkerTooWide { column : usize },
    /// The streaming writer has already been finished.
    Finished,
}

impl From<fmt::Error> for Error {
    fn from(_ : fmt::Error) -> Self { Self::Fmt }
}

/// Border positions shared by the fixed and automatic renderers.
#[derive(Clone, Copy)]
enum BorderKind {
    Top,
    Middle,
    Bottom,
}

fn validate_columns(columns : &[Column]) -> Result<(), Error> {
    for (column, spec) in columns.iter()
                                 .enumerate()
    {
        if spec.width == 0 && !matches!(spec.overflow, Overflow::Truncate("")) {
            return Err(Error::InvalidWidth { column });
        }
        if let Overflow::Truncate(marker) = spec.overflow {
            if UnicodeWidthStr::width(marker) > spec.width {
                return Err(Error::TruncationMarkerTooWide { column });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Measurement {
    width : usize,
}

enum MeasureError {
    Fmt,
    Multiline,
}

fn measure(cell : &Cell<'_>) -> Result<Measurement, MeasureError> {
    let mut writer = WidthWriter { width : 0,
                                   multiline : false };
    cell.format(&mut writer)
        .map_err(|_| MeasureError::Fmt)?;
    if writer.multiline {
        Err(MeasureError::Multiline)
    } else {
        Ok(Measurement { width : writer.width })
    }
}

fn validate_measurement(column : usize,
                        spec : &Column,
                        measurement : Measurement)
                        -> Result<(), Error> {
    if measurement.width > spec.width && matches!(spec.overflow, Overflow::Error) {
        return Err(Error::ContentOverflow { column,
                                            width : spec.width,
                                            actual : measurement.width });
    }
    Ok(())
}

struct WidthWriter {
    width : usize,
    multiline : bool,
}

impl Write for WidthWriter {
    fn write_str(&mut self, value : &str) -> fmt::Result {
        if value.contains(['\n', '\r']) {
            self.multiline = true;
        }
        self.width += UnicodeWidthStr::width(value);
        Ok(())
    }
}

fn write_cell<W : Write>(output : &mut W,
                         cell : &Cell<'_>,
                         spec : &Column,
                         measurement : Measurement)
                         -> Result<(), Error> {
    write_repeat(output, ' ', spec.padding_left)?;
    let visible_width = measurement.width
                                   .min(spec.width);
    let spare = spec.width - visible_width;
    let (before, after) = match spec.alignment {
        Alignment::Left => (0, spare),
        Alignment::Center => (spare / 2, spare - spare / 2),
        Alignment::Right => (spare, 0),
    };
    write_repeat(output, ' ', before)?;

    if measurement.width <= spec.width {
        cell.format(output)?;
    } else if let Overflow::Truncate(marker) = spec.overflow {
        let marker_width = UnicodeWidthStr::width(marker);
        let mut clipped = ClipWriter { output,
                                       remaining : spec.width - marker_width };
        cell.format(&mut clipped)?;
        clipped.output
               .write_str(marker)?;
    }

    write_repeat(output, ' ', after)?;
    write_repeat(output, ' ', spec.padding_right)?;
    Ok(())
}

struct ClipWriter<'a, W> {
    output : &'a mut W,
    remaining : usize,
}

impl<W : Write> Write for ClipWriter<'_, W> {
    fn write_str(&mut self, value : &str) -> fmt::Result {
        if self.remaining == 0 {
            return Ok(());
        }
        let width = UnicodeWidthStr::width(value);
        if width <= self.remaining {
            self.output
                .write_str(value)?;
            self.remaining -= width;
            return Ok(());
        }

        for ch in value.chars() {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0);
            if width > self.remaining {
                break;
            }
            self.output
                .write_char(ch)?;
            self.remaining -= width;
        }
        Ok(())
    }
}

fn write_repeat<W : Write>(output : &mut W, value : char, count : usize) -> fmt::Result {
    for _ in 0..count {
        output.write_char(value)?;
    }
    Ok(())
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;

    #[test]
    fn fixed_table_streams_display_and_debug() {
        let columns = [Column::new(3, Alignment::Right),
                       Column::new(8, Alignment::Left)];
        let mut output = String::new();
        let number = 7;
        let text = "run";
        let mut table = FixedTable::new(&columns).begin(&mut output)
                                                 .unwrap();
        table.row(&[Cell::display(&number),
                    Cell::debug(&text)])
             .unwrap();
        table.finish()
             .unwrap();
        assert_eq!(output,
                   "+-----+----------+\n|   7 | \"run\"    |\n+-----+----------+");
    }

    #[test]
    fn fixed_table_supports_explicit_separator_and_alignment() {
        let columns = [Column::new(5, Alignment::Center),
                       Column::new(4, Alignment::Right)];
        let mut output = String::new();
        let mut table = FixedTable::new(&columns).style(Style::modern())
                                                 .begin(&mut output)
                                                 .unwrap();
        table.row(&[Cell::text("x"),
                    Cell::text("12")])
             .unwrap();
        table.separator()
             .unwrap();
        table.row(&[Cell::text("abc"),
                    Cell::text("-")])
             .unwrap();
        table.finish()
             .unwrap();
        assert_eq!(output,
                   "┌───────┬──────┐\n│   x   │   12 │\n├───────┼──────┤\n│  abc  │    - \
                    │\n└───────┴──────┘");
    }

    #[test]
    fn truncate_is_unicode_width_safe() {
        let columns = [Column::new(5, Alignment::Left).overflow(Overflow::Truncate(">"))];
        let mut output = String::new();
        let mut table = FixedTable::new(&columns).begin(&mut output)
                                                 .unwrap();
        table.row(&[Cell::text("你好世界")])
             .unwrap();
        table.finish()
             .unwrap();
        assert_eq!(output,
                   "+-------+\n| 你好> |\n+-------+");
    }

    #[test]
    fn rejects_bad_rows_overflow_and_multiline() {
        let columns = [Column::new(2, Alignment::Left)];
        let mut output = String::new();
        let mut table = FixedTable::new(&columns).begin(&mut output)
                                                 .unwrap();
        assert_eq!(table.row(&[]),
                   Err(Error::ColumnCount { expected : 1,
                                            actual : 0 }));
        assert_eq!(table.row(&[Cell::text("long")]),
                   Err(Error::ContentOverflow { column : 0,
                                                width : 2,
                                                actual : 4 }));
        assert_eq!(table.row(&[Cell::text("a\nb")]),
                   Err(Error::MultilineUnsupported { column : 0 }));
    }

    #[test]
    fn auto_table_matches_compact_ascii_layout() {
        let data = [["Debian", "1.1.1.1"],
                    ["Arch",
                     "127.1.1.1"]];
        let mut output = String::new();
        AutoTable::new(&data).fmt(&mut output)
                             .unwrap();
        assert_eq!(output,
                   "+--------+-----------+\n| Debian | 1.1.1.1   |\n+--------+-----------+\n| \
                    Arch   | 127.1.1.1 |\n+--------+-----------+");
    }

    #[test]
    fn dashboard_shape_stays_fixed_width() {
        let columns = [Column::new(4, Alignment::Right).overflow(Overflow::Truncate(">")),
                       Column::new(14, Alignment::Right).overflow(Overflow::Truncate(">")),
                       Column::new(6, Alignment::Left).overflow(Overflow::Truncate(">")),
                       Column::new(9, Alignment::Right).overflow(Overflow::Truncate(">")),
                       Column::new(4, Alignment::Left).overflow(Overflow::Truncate(">")),
                       Column::new(6, Alignment::Right).overflow(Overflow::Truncate(">")),
                       Column::new(8, Alignment::Right).overflow(Overflow::Truncate(">"))];
        let mut output = String::new();
        let mut table = FixedTable::new(&columns).begin(&mut output)
                                                 .unwrap();
        table.row(&[Cell::text("CPU"),
                    Cell::text("Current Task"),
                    Cell::text("State"),
                    Cell::text("Q O/F/R"),
                    Cell::text("Rsch"),
                    Cell::text("Switch"),
                    Cell::text("Timer")])
             .unwrap();
        table.separator()
             .unwrap();
        for cpu in 0..4 {
            table.row(&[Cell::display(&cpu),
                        Cell::text("task-with-an-id-that-is-too-long"),
                        Cell::text("USER"),
                        Cell::text("1/2/3"),
                        Cell::text("-"),
                        Cell::text("42"),
                        Cell::text("100")])
                 .unwrap();
        }
        table.finish()
             .unwrap();

        assert_eq!(output.lines()
                         .count(),
                   8);
        assert!(output.lines()
                      .all(|line| UnicodeWidthStr::width(line) == 73));
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write_str(&mut self, _ : &str) -> fmt::Result { Err(fmt::Error) }
    }

    #[test]
    fn propagates_writer_failure() {
        let columns = [Column::new(1, Alignment::Left)];
        assert!(matches!(FixedTable::new(&columns).begin(&mut FailingWriter),
                         Err(Error::Fmt)));
    }

    #[test]
    fn error_is_debuggable_without_alloc() {
        let value = format!("{:?}", Error::Finished);
        assert_eq!(value, "Finished");
    }
}
