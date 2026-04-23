# WaterOS MM 验证与回归指南

本指南用于验证 `wateros-mm` 是否不仅“页表结构正确”，而且“分页启用后实际生效”。

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
  - `[self-test] mm self-test done`
- 且无 panic。

### 盲区说明
- 仅通过该阶段，仍不能证明硬件 MMU 已切换到新页表。

## 2. 分页启用生效（satp + sfence.vma）

### 目标
- 验证存在完整链路：构建页表 -> 写 `satp` -> `sfence.vma` -> 映射访存生效。

### 观测点
- 启动日志应出现：
  - `[self-test][paging] satp before=...`
  - `[self-test][paging] satp after=...`
  - `[self-test][paging] mapped probe write ok: va=... -> pa=...`
- 其中 `satp after` 必须等于 `target`。

### 解释
- 自检会为 `0x8000_0000..0x8800_0000` 建立恒等映射保障内核继续执行，
  同时额外建立一个高地址探针映射，写该虚拟地址后从对应物理地址读回。
- 如果读回值正确，说明地址翻译已经受页表控制，而非单纯结构体自检。

## 3. PageFault 回归（可选深度验证）

### 目标
- 验证分页异常路径可观测，至少可在 trap 日志中看到 `scause/sepc/stval`。

### 操作
- 在 `os/src/main.rs` 中将 `ENABLE_FAULT_PROBE` 改为 `true`。
- 重新运行：
  - `make rv_qemu_run_with_log`

### 通过标准
- 终端或 `qemu.log` 中出现：
  - `[trap] page fault: cause=... scause=... sepc=... stval=...`
- `stval` 应为触发探针的未映射虚拟地址。

## 4. 快速回归与深度诊断

### 快速回归
- `make rv_qemu_run`

### 深度诊断
- `make rv_qemu_run_with_log`（收集 `qemu.log`）
- `make rv_qemu_gdb` 与另一个终端 `make rv_gdb`，重点查看：
  - `satp`
  - `scause`
  - `stval`
  - `sepc`

## 5. 常见失败定位
- `satp after != target`：检查 `csrw satp` 调用路径和 feature 开关。
- 切页表后立刻异常：优先确认内核执行区间恒等映射是否完整。
- 探针写回不一致：确认探针 VA/PA 对应页是否映射且权限包含 `R|W`。
- 无 page fault 日志：确认 `ENABLE_FAULT_PROBE=true` 且 trap 初始化已执行。
