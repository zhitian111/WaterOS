use super::*;

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
