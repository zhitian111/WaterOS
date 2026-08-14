#![no_std]

#![forbid(unsafe_code)]
//! Allocation-free text table formatting over [`core::fmt`].
//!
//! The compact layout is derived from the MIT-licensed `tabled` 0.20.0 and
//! `papergrid` 0.17.0 compact renderers. See `UPSTREAM.md` in the package.

use core::fmt::{self, Debug, Display, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

/// Configuration for a fixed-width streaming table.
#[derive(Clone, Copy, Debug)]
pub struct FixedTable<'a> {
    columns : &'a [Column],
    style : Style,
}

impl<'a> FixedTable<'a> {
    /// Create a table from fixed-width columns.
    pub const fn new(columns : &'a [Column]) -> Self {
        Self { columns,
               style : Style::ascii() }
    }

    /// Select a border style.
    pub const fn style(mut self, style : Style) -> Self {
        self.style = style;
        self
    }

    /// Start streaming a table into `output`.
    pub fn begin<'w, W : Write>(self,
                                output : &'w mut W)
                                -> Result<FixedTableWriter<'a, 'w, W>, Error> {
        validate_columns(self.columns)?;
        let mut writer = FixedTableWriter { output,
                                            table : self,
                                            has_line : false,
                                            finished : false };
        if !writer.table
                  .columns
                  .is_empty()
        {
            writer.write_border(BorderKind::Top)?;
            writer.has_line = true;
        }
        Ok(writer)
    }
}

/// Active fixed-width table stream.
///
/// FORMAT_STATE: `begin` produces an active writer; `row` and `separator`
/// append to it; consuming `finish` writes the bottom border. The writer
/// borrows its destination, so an unfinished table cannot be written to from
/// another owner at the same time.
pub struct FixedTableWriter<'a, 'w, W> {
    output : &'w mut W,
    table : FixedTable<'a>,
    has_line : bool,
    finished : bool,
}

impl<W : Write> FixedTableWriter<'_, '_, W> {
    /// Write one row. Separators are explicit through [`Self::separator`].
    pub fn row(&mut self, cells : &[Cell<'_>]) -> Result<(), Error> {
        self.ensure_active()?;
        if cells.len() !=
           self.table
               .columns
               .len()
        {
            return Err(Error::ColumnCount { expected : self.table
                                                           .columns
                                                           .len(),
                                            actual : cells.len() });
        }

        for (column, (cell, spec)) in cells.iter()
                                           .zip(self.table.columns)
                                           .enumerate()
        {
            let measured =
                measure(cell).map_err(|kind| match kind {
                                 MeasureError::Fmt => Error::Fmt,
                                 MeasureError::Multiline => Error::MultilineUnsupported { column },
                             })?;
            validate_measurement(column, spec, measured)?;
        }

        self.newline()?;
        self.output
            .write_char(self.table
                            .style
                            .left)?;
        for (column, (cell, spec)) in cells.iter()
                                           .zip(self.table.columns)
                                           .enumerate()
        {
            let measured =
                measure(cell).map_err(|kind| match kind {
                                 MeasureError::Fmt => Error::Fmt,
                                 MeasureError::Multiline => Error::MultilineUnsupported { column },
                             })?;
            write_cell(self.output, cell, spec, measured)?;
            if column + 1 ==
               self.table
                   .columns
                   .len()
            {
                self.output
                    .write_char(self.table
                                    .style
                                    .right)?;
            } else {
                self.output
                    .write_char(self.table
                                    .style
                                    .vertical)?;
            }
        }
        self.has_line = true;
        Ok(())
    }

    /// Write a horizontal separator.
    pub fn separator(&mut self) -> Result<(), Error> {
        self.ensure_active()?;
        if self.table
               .columns
               .is_empty()
        {
            return Ok(());
        }
        self.newline()?;
        self.write_border(BorderKind::Middle)?;
        self.has_line = true;
        Ok(())
    }

    /// Finish the frame by writing its bottom border.
    pub fn finish(mut self) -> Result<(), Error> {
        self.ensure_active()?;
        if !self.table
                .columns
                .is_empty()
        {
            self.newline()?;
            self.write_border(BorderKind::Bottom)?;
        }
        self.finished = true;
        Ok(())
    }

    fn ensure_active(&self) -> Result<(), Error> {
        if self.finished {
            Err(Error::Finished)
        } else {
            Ok(())
        }
    }

    fn newline(&mut self) -> Result<(), Error> {
        if self.has_line {
            self.output
                .write_char('\n')?;
        }
        Ok(())
    }

    fn write_border(&mut self, kind : BorderKind) -> Result<(), Error> {
        let style = self.table.style;
        let (left, fill, intersection, right) = match kind {
            BorderKind::Top => (style.top_left, style.top, style.top_intersection, style.top_right),
            BorderKind::Middle => (style.left_intersection,
                                   style.horizontal,
                                   style.intersection,
                                   style.right_intersection),
            BorderKind::Bottom => {
                (style.bottom_left, style.bottom, style.bottom_intersection, style.bottom_right)
            }
        };
        self.output
            .write_char(left)?;
        for (index, column) in self.table
                                   .columns
                                   .iter()
                                   .enumerate()
        {
            write_repeat(self.output,
                         fill,
                         column.padding_left + column.width + column.padding_right)?;
            self.output
                .write_char(if index + 1 ==
                               self.table
                                   .columns
                                   .len()
                            {
                                right
                            } else {
                                intersection
                            })?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum BorderKind {
    Top,
    Middle,
    Bottom,
}

/// Automatic-width table for a rectangular array of strings.
///
/// Like upstream `CompactTable`, this consumes no heap memory and assumes
/// every cell is a single line.
#[derive(Clone, Copy, Debug)]
pub struct AutoTable<'a, T, const ROWS: usize, const COLS: usize> {
    records : &'a [[T; COLS]; ROWS],
    style : Style,
}

impl<'a, T, const ROWS: usize, const COLS: usize> AutoTable<'a, T, ROWS, COLS> {
    /// Create an automatically sized table.
    pub const fn new(records : &'a [[T; COLS]; ROWS]) -> Self {
        Self { records,
               style : Style::ascii() }
    }

    /// Select a border style.
    pub const fn style(mut self, style : Style) -> Self {
        self.style = style;
        self
    }
}

impl<T : AsRef<str>, const ROWS: usize, const COLS: usize> AutoTable<'_, T, ROWS, COLS> {
    /// Format the table into a [`Write`] destination.
    pub fn fmt<W : Write>(&self, output : &mut W) -> Result<(), Error> {
        if ROWS == 0 || COLS == 0 {
            return Ok(());
        }

        let mut widths = [0; COLS];
        for row in self.records
                       .iter()
        {
            for (column, value) in row.iter()
                                      .enumerate()
            {
                let text = value.as_ref();
                if text.contains(['\n', '\r']) {
                    return Err(Error::MultilineUnsupported { column });
                }
                widths[column] = widths[column].max(UnicodeWidthStr::width(text));
            }
        }

        write_auto_border(output,
                          &widths,
                          self.style,
                          BorderKind::Top)?;
        for (row_index, row) in self.records
                                    .iter()
                                    .enumerate()
        {
            output.write_char('\n')?;
            output.write_char(self.style.left)?;
            for (column, value) in row.iter()
                                      .enumerate()
            {
                let text = value.as_ref();
                output.write_char(' ')?;
                output.write_str(text)?;
                write_repeat(output,
                             ' ',
                             widths[column] - UnicodeWidthStr::width(text))?;
                output.write_char(' ')?;
                output.write_char(if column + 1 == COLS {
                                      self.style.right
                                  } else {
                                      self.style.vertical
                                  })?;
            }
            if row_index + 1 != ROWS {
                output.write_char('\n')?;
                write_auto_border(output,
                                  &widths,
                                  self.style,
                                  BorderKind::Middle)?;
            }
        }
        output.write_char('\n')?;
        write_auto_border(output,
                          &widths,
                          self.style,
                          BorderKind::Bottom)?;
        Ok(())
    }
}

fn write_auto_border<W : Write, const COLS: usize>(output : &mut W,
                                                   widths : &[usize; COLS],
                                                   style : Style,
                                                   kind : BorderKind)
                                                   -> Result<(), Error> {
    let (left, fill, intersection, right) = match kind {
        BorderKind::Top => (style.top_left, style.top, style.top_intersection, style.top_right),
        BorderKind::Middle => (style.left_intersection,
                               style.horizontal,
                               style.intersection,
                               style.right_intersection),
        BorderKind::Bottom => {
            (style.bottom_left, style.bottom, style.bottom_intersection, style.bottom_right)
        }
    };
    output.write_char(left)?;
    for (column, width) in widths.iter()
                                 .enumerate()
    {
        write_repeat(output, fill, width + 2)?;
        output.write_char(if column + 1 == COLS {
                              right
                          } else {
                              intersection
                          })?;
    }
    Ok(())
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
