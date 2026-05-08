# wateros-abi 功能快照

## 用途

记录 **`wateros-abi`** 中与用户态 / Linux riscv64 约定相关的 **no_std** 契约：errno、返回值包装、系统调用参数包、**`SyscallNumber`** 与号表 trait，以及可选 **`impl-*`** 提供的具体编号表。

## 事实来源

- `os/components/wateros-abi/Cargo.toml`
- `os/components/wateros-abi/src/lib.rs`
- `os/components/wateros-abi/abi-api/api-v0/`
- `os/components/wateros-abi/abi-impl/impl-linux-generic64/`、`abi-impl/impl-dummy/`

## Feature 与聚合导出

- **`default`**：`api-v0`、`impl-linux-riscv64`。
- **`api-v0`**：联动子 crate **`impl-dummy/api-v0`**（工作区成员约束）。
- **`impl-linux-generic64`**：启用 Linux asm-generic 64-bit 编号表实现（**`LinuxGeneric64`**）。
- **`impl-linux-riscv64`** / **`impl-linux-loongarch64`**：架构侧别名，等价于启用 **`impl-linux-generic64`**。
- **`impl-dummy`**：占位 impl crate，与号表无实质衔接。
- 聚合层在 **`api-v0`** 下导出 **`user_ret`**、**`errno`**、**`syscall_args`**、**`syscall_number`**；启用 **`impl-linux-generic64`** 时 **`ActiveSyscallNumberTable`** 指向 **`impl_linux_generic64::LinuxGeneric64`**。

## api-v0 契约要点

- **`errno`**：**`ErrNo`**、**`KernelResult`**、常用 Linux errno 常量。
- **`user_ret`**：**`UserRet`**、**`SyscallResult`**、与内核结果转换辅助。
- **`syscall_args`**：**`SyscallArgs`**、**`SyscallPacket`**（参数个数上界来自 **`wateros-base-config`** 的 **`MAX_SYSCALL_ARGS`**）。
- **`syscall_number`**：**`SyscallNumber`** newtype、**`SyscallNumberTable`** trait（按能力域分组的关联常量）。

## impl 层

- **`impl-linux-generic64`**：**真实**实现 **`SyscallNumberTable`**（与 Linux 64 位用户态约定对齐的子集）；RISC-V 与 LoongArch 早期路径复用同一张表。注释中说明当前子集面向 busybox / 简单进程，后续可按 strace 等补全。
- **`impl-dummy`**：**桩**，仅示例 **`add`** 与测试，**不**实现 **`SyscallNumberTable`**。

## 明确未覆盖

- 与 Linux generic 64 位表有显著差异的专用架构 **`impl-*`**（若未来从别名中拆出）。
- 将 **`impl-dummy`** 提升为可切换的完整号表后端（当前与 ABI 主路径无关）。

## 维护要求

号表子集范围、默认 feature 或聚合导出变化时，同步更新本文件与依赖 **`wateros-abi`** 的组件文档（如 **`wateros-syscall`**）。
