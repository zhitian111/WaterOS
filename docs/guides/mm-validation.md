# WaterOS MM 验证与回归指南

本指南用于验证 `wateros-mm` 是否不仅“页表结构正确”，而且“分页启用后实际生效”，以及与 **`mm::kernel_mm`**（内核全局页表 + 用户 ELF 装载）/ 用户态 `satp` 的衔接。

## 1. 基线检查（软件语义）

### 目标
- 确认 `mm::test_with_range(...)` 覆盖 map/protect/unmap/translate 基础行为。
- 确认 `impl-sv39` 负例（重复映射、未映射保护、重复 unmap）能被断言捕获。

### 执行
- 在 `os/` 目录运行：
  - `make rv_qemu_run`

### 通过标准
- 串口日志出现：
  - `[self-test] frame range ppn=[...)`
  - `[wateros-mm] test begin`
  - `[mm-impl::sv39] test begin`
  - `[kernel-mm] satp target=...`
  - `[kernel-mm] paging probe ok ...`
  - `[self-test] mm self-test done`
- 且无 panic。

### 盲区说明
- 仅通过 API 自检，仍依赖后续 `mm::kernel_mm::init`（实现位于 `os/components/wateros-mm/mm-impl/impl-sv39/`）完成硬件 `satp` 与探针验证。

## 2. 全局内核页表（`mm::kernel_mm`）

### 目标
- 根页表与中间表帧由帧分配器分配；根表通过 `Box::leak` 常驻，避免 `satp` 指向已 `Drop` 的页表（UAF）。
- `0x8000_0000..0x8800_0000` 恒等映射（S 态、`R|W|X`、无 `U`），保证内核与 trap 代码可执行。

### 观测点
- `[kernel-mm] satp target=...`
- `[kernel-mm] paging probe ok va=... -> pa=...`（高地址探针写回与 `translate_addr` 一致）。

## 3. trap 返回前 `satp`（`KERNEL_TRAP_SATP` 与用户地址空间）

### 行为
- 内核在 `mm::kernel_mm::init` 之后应调用 `task::init_kernel_trap_satp(mm::kernel_mm::kernel_satp())`，将内核 `satp` 记入调度/ trap 路径。
- `__wateros_task_runtime_install_trap_satp`：返回用户态前写入当前任务的 `AddressSpaceHandle::raw()`；`raw == 0` 表示沿用内核 `satp`。
- 从用户态 trap 返回内核态时写回 `KERNEL_TRAP_SATP`。

## 4. 用户栈与 stage4

### `UserTaskSpec::with_external_stack`
- 由 loader 或 `mm::kernel_mm::map_anon_range_user` 预先映射的用户栈虚拟区间 `(bottom, top]`，不再使用内核堆上的 `UserStack` 物理页（避免缺 `U` 位导致用户态不可访问）。

### stage4 自检
- 对落在内核镜像内的用户入口页调用 `mm::kernel_mm::ensure_user_execute_for_kernel_va`，为该 VPN 增加 `U` 以便用户态执行。
- 高地址匿名栈由 `mm::kernel_mm::map_anon_range_user` 映射。
- `AddressSpaceHandle::from_raw(mm::kernel_mm::kernel_satp())` 与 `raw == 0` 约定一致：共享全局内核页表做演示。

## 5. ELF 装载（`from_elf_path`）

### 路径
- 默认尝试 `mm::kernel_mm::DEFAULT_USER_ELF_PATH`（定义于 `wateros-mm` 的 `mm-api/api-v0/src/kernel_bringup.rs`，根卷内）；不存在时仅 `[elf-selftest] skip` 警告，不阻塞启动。

### 行为
- 独立 `Sv39AddressSpace`（同样 `Box::leak`）：含内核 RAM 恒等映射 + `PT_LOAD` 段 + 用户栈；`satp` 与 `entry_pc` 交给 `spawn_user_task_spec`。

## 6. PageFault 回归（可选）

- trap 中对 page fault 打 `[trap] page fault: ...` 日志；未处理时仍会继续恢复路径（开发中可自行改为 `panic`）。

## 7. 快速回归与深度诊断

### 快速回归
- `make rv_qemu_run`

### 深度诊断
- `make rv_qemu_run_with_log`（收集 `qemu.log`）
- `make rv_qemu_gdb` 与另一个终端 `make rv_gdb`，重点查看：
  - `satp`
  - `scause`
  - `stval`
  - `sepc`

## 8. 常见失败定位
- `kernel_mm: not initialized`：确认 `mm::kernel_mm::init` 在 `task::init` 之前调用。
- 用户态一进入即 fault：检查入口页是否含 `U|X`，栈区间是否含 `U|R|W`。
- `satp` 与用户程序不一致：确认 `AddressSpaceHandle::raw()` 与独立 ELF 页表的 `satp_value()` 一致。
