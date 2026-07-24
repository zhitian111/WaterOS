# wateros-table-format

An allocation-free, `#![no_std]` text-table formatter. It writes to any
`core::fmt::Write` implementation and never accesses stdout, a terminal, or an
operating-system API.

The crate offers two paths:

- `FixedTable` streams rows of borrowed `Display`, `Debug`, or string cells
  using caller-specified column widths.
- `AutoTable` computes column widths for a rectangular array of values that
  implement `AsRef<str>`.

```rust
use wateros_table_format::{Alignment, Cell, Column, FixedTable, Style};

let columns = [
    Column::new(3, Alignment::Right),
    Column::new(8, Alignment::Left),
];
let mut output = String::new();
let mut table = FixedTable::new(&columns)
    .style(Style::ascii())
    .begin(&mut output)?;
table.row(&[Cell::display(&7), Cell::text("running")])?;
table.finish()?;
# Ok::<(), wateros_table_format::Error>(())
```

Values formatted through `Display` or `Debug` may be formatted twice per row
so the implementation can align and truncate without allocating. Their output
must therefore remain stable for the duration of a `row` call. Multiline cell
content is rejected.

