# wateros-utils

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-utils` 是与 WaterOS 内核策略、平台和全局状态无关的 `#![no_std]` 工具入口。目前
公共 API 只有 `table_format` 的原样重导出。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | `no_std` 纯工具聚合入口，原样重导出 `table_format`。 |
| 表格格式化 | `table-format/` | 固定列宽或自动列宽的文本表格格式化（`Alignment` / `Cell` / `Column` / `FixedTable`）。 |

## 实现说明

- 本 crate 可以承载确定性的纯函数、小型数据结构和格式化工具；它**不能**依赖 task、MM、driver
  或 platform。
- 启动汇编、UART 直写、CSR/MMU 操作必须放在 `wateros-platform` 的 arch/profile 实现中，避免
  utils 反向依赖平台。
- `table-format` 无分配、不写串口、单元格不能含换行；格式化结果写入调用方提供的
  `core::fmt::Write`。
- 此前未被构建系统引用的 RISC-V UART 寄存器打印汇编及模板 `add` 函数已删除，它们都不是可维护
  的公共 API。

## 调用链路

```rust
use utils::table_format::{Alignment, Cell, Column, FixedTable};
// 构造表格后写入调用方提供的 core::fmt::Write
```

表格工具写入 `core::fmt::Write`。例如 dashboard 应在持有自己的输出序列化锁时先完成字符串构造，
再一次性输出，避免与其它 CPU 的日志交错。

## 各实现功能

### table-format / 文本表格

主要实现在 `table-format/src/lib.rs`（紧凑布局派生自 MIT 许可的 `tabled` 0.20.0 /
`papergrid` 0.17.0，见 `UPSTREAM.md`）。

- `Alignment`：`Left`（值后补 padding）/ `Center`（两侧均分）/ `Right`（值前补 padding）。
- `Overflow`：值宽于列时的行为——`Error`（写行前返回 `Error::ContentOverflow`）/
  `Truncate(marker)`（裁剪并追加标记）。
- `Column`：`width`（内容宽度，不含 padding）、`alignment`、`padding_left`/`padding_right`
  （默认 1）、`overflow`（默认 `Error`）；`Column::new(width, alignment)` 与 `padding`/
  `overflow` 构建器。
- `FixedTable`：固定列宽或自动列宽的文本表格，写入调用方提供的 `core::fmt::Write`；
  `#![forbid(unsafe_code)]`、无堆分配，单元格不能含换行。

### src / 聚合入口

- `lib.rs`：`#![no_std]` 纯工具聚合入口，仅导出 `table_format`。
- `asm/`：占位目录（未引用的汇编不进入公共 API）。

## 回归入口

运行table-format单测覆盖左右/居中、零宽、Unicode显示宽度、padding、Error/Truncate及fmt writer失败。聚合层保持no_std和无平台依赖；新增工具后用依赖树验证没有反向引入task/MM/driver。
