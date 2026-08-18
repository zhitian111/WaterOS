use super::*;

pub struct FixedTable<'a> {
    /// 固定宽度列定义。
    columns : &'a [Column],
    /// 边框样式。
    style : Style,
}

impl<'a> FixedTable<'a> {
    /// 根据固定宽度列定义创建表格。
    pub const fn new(columns : &'a [Column]) -> Self {
        Self { columns,
               style : Style::ascii() }
    }

    /// 选择边框样式。
    pub const fn style(mut self, style : Style) -> Self {
        self.style = style;
        self
    }

    /// 开始将表格流式写入 `output`，并校验列定义。
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

/// 活跃的固定宽度表格流。
///
/// FORMAT_STATE: `begin` produces an active writer; `row` and `separator`
/// 逐行追加内容；消费 `finish` 会写入底部边框。写入器借用目标缓冲区，
/// 因此表格未完成时，其他所有者不能同时写入该缓冲区。
pub struct FixedTableWriter<'a, 'w, W> {
    /// 输出目标的可变借用。
    output : &'w mut W,
    /// 表格定义与样式。
    table : FixedTable<'a>,
    /// 是否已经写出至少一行边框或数据。
    has_line : bool,
    /// 是否已调用 `finish`；完成后不能继续写入。
    finished : bool,
}

impl<W : Write> FixedTableWriter<'_, '_, W> {
    /// 写入一行；分隔线通过 [`Self::separator`] 显式控制。
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

    /// 写入水平分隔线。
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

    /// 写入底部边框并完成表格。
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
