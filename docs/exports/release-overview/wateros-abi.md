# wateros-abi — 阶段能力概述

## 当前阶段目标

为 WaterOS 用户态程序（busybox、glibc/musl 工具链）提供与 Linux 用户 ABI 对齐的 syscall 边界类型，使内核分发层与用户返回值编码保持一致。

## 已具备

- **错误与返回值语义**：`ErrNo` 常量子集、`UserRet` 成功/失败编码，与 Linux `-errno` 约定一致
- **参数传递布局**：`repr(C)` 的 `SyscallArgs` / `SyscallPacket`，槽位与陷阱帧对齐
- **调用号抽象**：`SyscallNumber` newtype 与 `SyscallNumberTable` trait，覆盖早期用户态所需的文件、进程、内存、信号、socket 等符号
- **生产号表**：`impl-linux-generic64` 在 riscv64 与 loongarch64 主线上启用，提供 `ActiveSyscallNumberTable`
- **质量护栏**：号表唯一性编译期断言与单测

## 适用范围

- QEMU `virt` 上跑 busybox / LTP / lmbench 等用户态 bring-up
- 需要与 libc 调用号对齐的内核 syscall 实现与测试

## 已知限制

- ABI 聚合 crate 默认 feature 为空，须通过内核 feature 链启用
- 号表为早期子集；trait 中有名无 impl 的 syscall 仍会 `ENOSYS`
- RISC-V 与 LoongArch 暂共用一张 asm-generic 表
- `impl-dummy` 仅为构建占位，无运行时价值

## 下一步方向（未承诺）

- 按架构拆分专用号表
- 扩展 `SyscallNumberTable` 覆盖更多 LTP 缺口
- 视需要引入 api-v1 或独立用户态 crate 重导出
