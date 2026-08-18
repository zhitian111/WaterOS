#![no_std]

#![forbid(unsafe_code)]
//! 基于 [`core::fmt`] 的无分配文本表格格式化。
//!
//! 紧凑布局参考 MIT 许可的 `tabled` 0.20.0 与 `papergrid` 0.17.0 渲染器；
//! 详见包内 `UPSTREAM.md`。

use core::fmt::{self, Debug, Display, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod fixed_table;
pub use fixed_table::{FixedTable, FixedTableWriter};
mod auto_table;
pub use auto_table::AutoTable;

/// 列内容区域内的水平对齐方式。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Alignment {
    /// 在值后放置填充。
    Left,
    /// 在值两侧分配填充。
    Center,
    /// 在值前放置填充。
    Right,
}

/// 格式化值超过列宽时的处理方式。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overflow {
    /// 在写入行之前返回 [`Error::ContentOverflow`]。
    Error,
    /// 截断内容并追加此标记。
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
    /// 创建列；`width` 只计算内容单元格宽度，不包含内边距。
    pub const fn new(width : usize, alignment : Alignment) -> Self {
        Self { width,
               alignment,
               padding_left : 1,
               padding_right : 1,
               overflow : Overflow::Error }
    }

    /// 设置左右内边距。
    pub const fn padding(mut self, left : usize, right : usize) -> Self {
        self.padding_left = left;
        self.padding_right = right;
        self
    }

    /// 设置溢出处理策略。
    pub const fn overflow(mut self, overflow : Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// 返回内容宽度。
    pub const fn width(&self) -> usize { self.width }
}

/// 表格使用的边框字符。
///
/// 这是 `papergrid` 使用的紧凑边框模型：包含外框和水平分隔线，
/// 每个交汇位置使用一个交叉字符。
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

    /// Unicode 方框绘制风格。
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

/// 借用的单元格值。
#[derive(Clone, Copy)]
pub enum Cell<'a> {
    /// 字符串单元格。
    Text(&'a str),
    /// 使用 [`Display`] 格式化的值。
    Display(&'a dyn Display),
    /// 使用 [`Debug`] 格式化的值。
    Debug(&'a dyn Debug),
}

impl<'a> Cell<'a> {
    /// 构造字符串单元格。
    pub const fn text(value : &'a str) -> Self { Self::Text(value) }

    /// 构造 [`Display`] 单元格。
    pub fn display<T : Display>(value : &'a T) -> Self { Self::Display(value) }

    /// 构造 [`Debug`] 单元格。
    pub fn debug<T : Debug>(value : &'a T) -> Self { Self::Debug(value) }

    fn format(&self, output : &mut dyn Write) -> fmt::Result {
        match self {
            Self::Text(value) => output.write_str(value),
            Self::Display(value) => write!(output, "{value}"),
            Self::Debug(value) => write!(output, "{value:?}"),
        }
    }
}

/// 表格格式化失败。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// 目标输出端拒绝写入。
    Fmt,
    /// 行中的单元格数量与列数不一致。
    ColumnCount { expected : usize, actual : usize },
    /// A zero-width column cannot represent its configured truncation marker.
    InvalidWidth { column : usize },
    /// 单元格包含换行符。
    MultilineUnsupported { column : usize },
    /// 单元格超出配置为 [`Overflow::Error`] 的列宽。
    ContentOverflow {
        column : usize,
        width : usize,
        actual : usize,
    },
    /// 截断标记本身宽于可用内容区域。
    TruncationMarkerTooWide { column : usize },
    /// 流式写入器已经完成，不能再次写入。
    Finished,
}

impl From<fmt::Error> for Error {
    fn from(_ : fmt::Error) -> Self { Self::Fmt }
}

/// 固定宽度和自动宽度渲染器共用的边框位置。
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
