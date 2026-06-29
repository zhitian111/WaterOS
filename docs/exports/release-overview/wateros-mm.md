# wateros-mm 版本概述

## 定位

`wateros-mm` 是 WaterOS 的**内存管理一级组件**，为内核 bring-up、用户进程装载、syscall 内存语义（brk/mmap/mprotect/mremap/madvise）以及 fork 写时复制提供统一入口。对外以聚合 crate `wateros-mm` 暴露；内部按 **API 契约 + 平台 impl + 物理帧分配** 分层，避免页表细节泄漏到 syscall 以外模块。

## 当前阶段已具备的能力

- **4 KiB 分页**的完整用户态内存模型（堆、匿名/文件 mmap、栈按需扩页）。
- **RISC-V Sv39** 与 **LoongArch64** 两套三级页表实现，经根 feature 二选一。
- **内核恒等映射** bring-up（RAM + MMIO），与用户地址空间分离。
- **ELF64 小端**装载、动态链接解释器、shebang 脚本转解释器路径。
- **fork 写时复制**与共享匿名映射。
- **惰性映射**（大段私有匿名 mmap、文件 mmap）以降低物理帧峰值。
- **栈式物理帧分配器**（带引用计数），支撑 COW 与 `/proc` 内存统计。
- **单节点 NUMA 策略桩**，满足基础 syscall 探测。

## 适用范围

- QEMU **riscv64 virt + OpenSBI**（`wateros` feature `impl-sv39` / `qemu-riscv64-opensbi`）。
- QEMU **loongarch64 virt**（`qemu-loongarch64-virt`）。
- 仅编译 API、不链真实页表时可用 `wateros-mm` 默认 `api-v0` + `impl-dummy`（不运行用户 ELF）。

## 设计取舍（本阶段）

- 固定 **4 KiB** 页；无 transparent huge page。
- 页表 walk 假设 **物理地址可内核直访**（恒等映射 bring-up）。
- 用户地址空间以 **泄漏裸指针** 交给 task，换取 syscall 路径零额外装箱。
- 帧分配为 **单核栈式** 实现，适合当前 uniprocessor bring-up，非生产级 buddy 分配器。
- mmap/mremap/mempolicy 为 **Linux 语义子集**，以满足 LTP/glibc 主线为主。

## 后续演进方向（非本快照承诺）

- 多核安全的帧分配与 TLB shootdown 策略。
- 非恒等内核映射模型下重写 `table_mut` 物理访问路径。
- 完整 SHARED 文件映射与更严格的 VMA 元数据。
- 真实 NUMA 拓扑与 per-vma 策略状态。
