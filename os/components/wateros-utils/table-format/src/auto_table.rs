use super::*;

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

