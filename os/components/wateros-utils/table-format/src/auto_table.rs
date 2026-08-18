use super::*;

pub struct AutoTable<'a, T, const ROWS: usize, const COLS: usize> {
    /// 借用的固定行列数据，不复制单元格内容。
    records : &'a [[T; COLS]; ROWS],
    /// 边框样式。
    style : Style,
}

impl<'a, T, const ROWS: usize, const COLS: usize> AutoTable<'a, T, ROWS, COLS> {
    /// 创建按单元格内容自动计算列宽的表格。
    pub const fn new(records : &'a [[T; COLS]; ROWS]) -> Self {
        Self { records,
               style : Style::ascii() }
    }

    /// 选择边框样式。
    pub const fn style(mut self, style : Style) -> Self {
        self.style = style;
        self
    }
}

impl<T : AsRef<str>, const ROWS: usize, const COLS: usize> AutoTable<'_, T, ROWS, COLS> {
    /// 将表格格式化写入 [`Write`] 目标；包含换行的单元格会返回错误。
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
