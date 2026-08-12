# wateros-table-format

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-utils](../README.md)

一个无堆分配、`#![no_std]` 的文本表格格式化器。它只写入调用方提供的
`core::fmt::Write`，从不直接访问 stdout、终端或操作系统 API。

## 数据结构与使用路径

- `Column` 保存单列的内容宽度、对齐、左右 padding 和溢出策略；`width` 不含
  padding，以显示单元格宽度而非 UTF-8 字节数计。
- `Cell` 借用字符串、`Display` 或 `Debug` 值，不拥有数据。
- `FixedTable` 由调用方指定列宽，`begin` 返回 `FixedTableWriter` 流式写行；
  适合 dashboard 等列宽固定的热路径。
- `AutoTable` 接受矩形 `[[T; COLS]; ROWS]`，以 `AsRef<str>` 值计算列宽；
  适合小型静态表。

`FixedTableWriter` 有明确状态：`begin → row/separator* → finish`。`finish` 消耗
writer 并写入底边框，因而不能在完成后继续追加行。

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

## 限制与错误语义

通过 `Display` 或 `Debug` 的值每一行可能会被格式化两次：一次测量 Unicode 显示宽度，
一次实际输出。因此同一次 `row` 调用期间它的格式化结果必须稳定，且不应有副作用。

单元格不能含换行。`Overflow::Error` 会在写入该行前返回
`Error::ContentOverflow`；`Overflow::Truncate(marker)` 按 Unicode 显示宽度裁剪后附加
标记。写入目标报错会转为 `Error::Fmt`。

本 crate 不负责并发输出的互斥：多核日志或 dashboard 的调用方必须自行在完整表格
字符串的最终输出处序列化。
