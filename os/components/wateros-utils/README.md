# wateros-utils

与 WaterOS 内核策略、平台和全局状态无关的 `#![no_std]` 工具入口。

目前公共 API 只有 `table_format` 的原样重导出：

```rust
use utils::table_format::{Alignment, Cell, Column, FixedTable};
```

## 依赖边界

本 crate 可以承载确定性的纯函数、小型数据结构和格式化工具；它不能依赖 task、MM、
driver 或 platform。启动汇编、UART 直写、CSR/MMU 操作必须放在
`wateros-platform` 的 arch/profile 实现中，避免 utils 反向依赖平台。

此前未被构建系统引用的 RISC-V UART 寄存器打印汇编，以及模板 `add` 函数已删除；
它们都不是可维护的公共 API。

## 子 crate

| 路径 | 职责 | 重要限制 |
| --- | --- | --- |
| `table-format/` | 固定列宽或自动列宽的文本表格格式化 | 无分配；不写串口；单元格不能含换行 |

表格工具会写入调用方提供的 `core::fmt::Write`。例如 dashboard 应在持有自己的输出
序列化锁时先完成字符串构造，再一次性输出，避免与其它 CPU 的日志交错。

