# wateros-abi 功能快照

## 用途

记录 **`wateros-abi`** 中与用户态 / Linux riscv64 约定相关的 **no_std** 契约：errno、返回值包装、系统调用参数包、**`SyscallNumber`** 与号表 trait，以及可选 **`impl-*`** 提供的具体编号表。

## 事实来源

- `os/components/wateros-abi/Cargo.toml`
- `os/components/wateros-abi/src/lib.rs`
- `os/components/wateros-abi/abi-api/api-v0/`
- `os/components/wateros-abi/abi-impl/impl-linux-riscv64/`、`abi-impl/impl-dummy/`

## Feature 与聚合导出

- **`default`**：`api-v0`、`impl-linux-riscv64`。
- **`api-v0`**：联动子 crate **`impl-dummy/api-v0`**（工作区成员约束）。
- **`impl-linux-riscv64`**：启用 Linux riscv64 编号表实现。
- **`impl-dummy`**：占位 impl crate，与号表无实质衔接。
- 聚合层在 **`api-v0`** 下导出 **`user_re`**、**`errno`**、**`syscall_args`**、**`syscall_number`**；启用 **`impl-linux-riscv64`** 时 **`ActiveSyscallNumberTable`** 指向 **`impl_linux_riscv64::LinuxRiscv64`**。

## api-v0 契约要点

- **`errno`**：**`ErrNo`**、**`KernelResult`**、常用 Linux errno 常量。
- **`user_ret`**：**`UserRet`**、**`SyscallResult`**、与内核结果转换辅助。
- **`syscall_args`**：**`SyscallArgs`**、**`SyscallPacket`**（参数个数上界来自 **`wateros-base-config`** 的 **`MAX_SYSCALL_ARGS`**）。
- **`syscall_number`**：**`SyscallNumber`** newtype、**`SyscallNumberTable`** trait（按能力域分组的关联常量）。

## impl 层

- **`impl-linux-riscv64`**：**真实**实现完整 **`SyscallNumberTable`**（riscv64 Linux 编号）；可提供 **`Glibc`** / **`Musl`** 类型别名指向同表。注释中说明当前子集面向 busybox / 简单进程，后续可按 strace 等补全。
- **`impl-dummy`**：**桩**，仅示例 **`add`** 与测试，**不**实现 **`SyscallNumberTable`**。

## 明确未覆盖

- 除 Linux riscv64 外的其它 ABI / 架构 **`impl-*`**。
- 将 **`impl-dummy`** 提升为可切换的完整号表后端（当前与 ABI 主路径无关）。

## 维护要求

号表子集范围、默认 feature 或聚合导出变化时，同步更新本文件与依赖 **`wateros-abi`** 的组件文档（如 **`wateros-syscall`**）。
