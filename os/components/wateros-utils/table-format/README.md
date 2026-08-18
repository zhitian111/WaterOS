# wateros-table-format

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-utils](../README.md)

这是 WaterOS 的无堆分配、`#![no_std]` 文本表格格式化器。它只依赖
`core::fmt` 和 `unicode-width`，只向调用方提供的 `fmt::Write` 写数据，不直接访问
UART、终端或操作系统 API。当前内核的 CPU dashboard 使用固定宽度接口；这个 crate
也可用于 `/proc` 调试输出、启动诊断和测试报告。

## 1. 模块边界和文件

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | 公共类型、列校验、显示宽度测量、单元格对齐/截断 |
| `src/fixed_table.rs` | 固定列宽、逐行流式输出和 writer 状态机 |
| `src/auto_table.rs` | 编译期固定行列数的二维字符串表，自动计算列宽 |
| `UPSTREAM.md` | `tabled`/`papergrid` 来源、版本、校验和及 WaterOS 裁剪说明 |

本 crate 不负责缓冲区分配、输出互斥、ANSI 转义、跨行单元格、合并单元格或终端宽度
探测。调用方必须决定存储位置及最终输出方式。

## 2. 公共数据结构

### `Column`

`Column` 保存一列的全部静态布局：

- `width`：内容区显示宽度，不含左右 padding；
- `alignment`：`Left`、`Center` 或 `Right`；
- `padding_left`、`padding_right`：默认各 1 个 ASCII 空格；
- `overflow`：超宽时报错，或裁剪并追加静态 marker。

`Column::new(width, alignment)` 是 `const fn`，`padding` 和 `overflow` 也可在静态数组中
配置。总列宽是 `padding_left + width + padding_right`，外框和列分隔符另计。

### `Cell<'a>`

`Cell` 不拥有数据，三个变体分别借用 `&str`、`&dyn Display`、`&dyn Debug`。因此：

- cell 数组只需活到当前 `row` 调用结束；
- 被借用值不得在格式化期间并发突变；
- `Display`/`Debug` 实现不得依赖“一次且仅一次调用”的副作用。

### `Style`

`Style` 包含上/中/下边框的 15 个字符。当前公开构造器为 `ascii()` 和 `modern()`；
字段私有，因此 crate 外不能任意拼装风格。如果需要新增样式，应在本 crate 内增加
`const fn`，同时添加逐字节或逐字符期望输出测试。

### `FixedTable`、`FixedTableWriter`

`FixedTable` 借用 `&[Column]`；`begin` 校验列并借用输出目标，返回活动 writer。
writer 同时持有列描述和 `&mut W`，Rust 借用关系保证表未结束时另一所有者不能写同一
目标，但这不替代跨 CPU/跨任务的外部锁。

### `AutoTable<'a, T, ROWS, COLS>`

它借用 `&[[T; COLS]; ROWS]`，要求 `T: AsRef<str>`。列宽数组 `[usize; COLS]` 放在栈上，
没有 `Vec` 或字符串复制。`ROWS == 0` 或 `COLS == 0` 时直接成功且不输出任何字节。

## 3. 显示宽度规则

所有宽度都通过 `UnicodeWidthStr::width` / `UnicodeWidthChar::width` 计算，语义是终端
显示列数，而非 UTF-8 字节数或 Rust `char` 个数。例如常见汉字通常占 2 列。组合字符
可能占 0 列；无法确定宽度的单个字符在裁剪器中按 0 处理。

这里没有解析 ANSI 控制序列。带颜色转义的文本会得到不符合肉眼效果的宽度，不应直接
作为 cell；应在表格生成后着色，或先扩展一个能识别转义序列的测量层。

换行 `\n` 和回车 `\r` 均被拒绝。其他控制字符不会被专门过滤，调用方不应把未经
清洗的用户输入直接用于控制台表格。

## 4. 固定宽度调用链

```text
FixedTable::new(columns).style(style)
  -> begin(output)
     -> validate_columns
     -> 非空列集：write_border(Top)
     -> FixedTableWriter
        -> row(cells)* / separator()*
        -> finish(self)
           -> 非空列集：write_border(Bottom)
```

一次 `row` 分两阶段：

1. 检查 cell 数必须恰好等于列数；
2. 对每个 cell 调用一次格式化以测宽，并检查换行与 `Overflow::Error`；
3. 只有整行逻辑校验成功后才写换行和左边框；
4. 对每个 cell 再测量一次，然后实际格式化、对齐并写分隔符。

所以列数、换行和内容超宽错误不会写出半行；但 `fmt::Write` 在实际输出中途返回错误时，
目标内可能已有边框或行前缀，接口不提供事务回滚。需要原子发布时，应先写入调用方的
临时 buffer，成功后再在一把输出锁内提交。

`separator()` 只在调用点写中间横线，不会自动把首行当表头。`finish(self)` 消耗 writer；
空列集的 `begin`、`separator`、`finish` 都不产生输出。

## 5. 对齐和截断算法

若实际宽度不超过内容宽度，空余 `spare = column.width - actual`：

- 左对齐：`before=0, after=spare`；
- 居中：`before=spare/2, after=spare-spare/2`，奇数空格偏右；
- 右对齐：`before=spare, after=0`。

超宽且策略为 `Overflow::Error` 时，返回 `ContentOverflow`。策略为
`Overflow::Truncate(marker)` 时，正文预算为 `column.width - marker_width`：裁剪器按
Unicode 标量边界写完整字符，绝不会切断 UTF-8 字节序列，最后追加 marker。超宽内容
已经占满列宽，因此对齐不会额外生效。

特殊合法配置是宽度 0 且 marker 为空字符串；任何其他宽度 0 配置返回 `InvalidWidth`。
marker 显示宽度超过内容区则在 `begin` 返回 `TruncationMarkerTooWide`。

## 6. 自动宽度调用链

```text
AutoTable::fmt(output)
  -> 遍历所有记录：拒绝换行并求每列最大 Unicode 显示宽度
  -> write_auto_border(Top)
  -> 逐行写 cell；每行之间自动写 Middle
  -> write_auto_border(Bottom)
```

每个 cell 在测量阶段和输出阶段各调用一次 `AsRef<str>`。自动表始终左右各补一个空格、
左对齐、不截断；大输入会生成相应的大表，调用方负责限制 `ROWS`、`COLS` 和文本长度。

## 7. 错误语义速查

| 错误 | 触发点 | 修复方向 |
|---|---|---|
| `Fmt` | 下游 writer 拒绝写入，或 cell 格式化返回 `fmt::Error` | 检查缓冲容量/设备状态和自定义 formatter |
| `ColumnCount` | `row` 的 cell 数和列数不同 | 同步列定义与行构造 |
| `InvalidWidth` | 零宽列不能表达配置 | 增加宽度，或使用空 marker 的 truncate |
| `MultilineUnsupported` | cell 含 `\n` 或 `\r` | 预先替换/拆分文本 |
| `ContentOverflow` | 内容超宽且策略为 `Error` | 扩列或配置 truncate |
| `TruncationMarkerTooWide` | marker 比内容区宽 | 缩短 marker 或扩列 |
| `Finished` | 内部活动状态检查发现已结束 | 正常安全 API 中 `finish` 消耗 self，通常不可达 |

## 8. 内核 dashboard 实际链路

`src/dashboard.rs` 中的真实调用关系是：

```text
dashboard_task
  -> render_snapshot() 分配一个 String
     -> FixedTable::begin(&mut frame)
     -> 表头 / separator / 每个 online CPU 一行 / finish
  -> runtime::console::write_raw_bytes(frame.as_bytes())
  -> task::sleep_for_ticks(50)
```

表格 crate 自身不分配，但 dashboard 的 `String::with_capacity(2048)` 会使用内核堆。
dashboard 刻意在普通内核任务而非 timer trap 中渲染，以避免中断上下文长时间占用 UART。
当前调用点忽略表格错误；修改列宽或表头时必须运行固定宽度测试，否则错误会表现为面板
缺行而不是 panic。

## 9. 新增诊断表实例

下面适合用于固定上限的 `/proc` 快照或内核调试缓冲区：

```rust
use wateros_table_format::{Alignment, Cell, Column, FixedTable, Overflow};

let columns = [
    Column::new(6, Alignment::Right),
    Column::new(16, Alignment::Left).overflow(Overflow::Truncate("…")),
];
let mut table = FixedTable::new(&columns).begin(output)?;
table.row(&[Cell::text("PID"), Cell::text("COMMAND")])?;
table.separator()?;
for task in tasks {
    table.row(&[Cell::display(&task.pid), Cell::text(task.name)])?;
}
table.finish()?;
```

若数据来自用户态，应先限制条目数并清理控制字符。不要持有调度器、VFS inode 或地址
空间写锁进行慢速 UART 输出；应在短锁内复制快照，释放锁后格式化。

## 10. 扩展检查表

新增能力时逐项确认：

1. 保持 `no_std`，明确是否仍为零堆分配；
2. 测量路径与实际输出路径必须使用相同的宽度规则；
3. 任何裁剪必须停在 UTF-8 字符边界，marker 也要计显示宽度；
4. 写之前尽量完成所有可预检的逻辑错误；
5. 不要在 formatter 中获取与最终输出锁相反顺序的锁；
6. 为 ASCII、中文/宽字符、组合字符、空表、writer 失败和边界宽度增加测试；
7. 若引入多行或 ANSI 支持，必须重新定义行高、边框及宽度算法，不能只放宽换行检查。

## 11. 自回归

从 `os/` 执行：

```sh
cargo test --manifest-path components/wateros-utils/table-format/Cargo.toml
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

单元测试至少覆盖 fixed/auto 输出、对齐、显式分隔线、Unicode 安全截断、错误行预检、
dashboard 固定行宽和 writer 故障传播。改动 `unicode-width` 版本时还应重新检查目标终端
对东亚宽字符/组合字符的显示是否符合预期。
