# 资源生命周期修复 — 新对话续接提示词

> 用途：复制下方「Agent 提示词」整段到新对话，让下一个 Agent 从当前进度继续推进代码修复。  
> 最后更新：2026-06-25

---

## Agent 提示词（复制以下全文）

你是 WaterOS 协作 Agent。请**继续推进资源生命周期审计的代码修复**，不要重复已完成的 P0 项。

### 背景

项目已完成资源审计，产出在 `docs/audits/`。前五波 P0 修复（clone 回滚、fd 账本、MAP_SHARED、futex/shm、unix socket、页缓存/umount 等）**已实现**，`make rv_check` 可通过。详见 `docs/audits/resource-fix-queue.md` 文末「建议实施顺序」。

### 你的任务（按优先级）

1. **P0 剩余**：`T-KH-01` — 内核堆 OOM 可恢复（`alloc_error_handler` 勿全局 panic；spawn/fork/mmap 等关键路径返回 `ENOMEM`）
2. **P1 队列**（见 `resource-fix-queue.md` §P1），建议顺序：
   - `T-PF-04/05/06`（brk ENOMEM、mmap partial 回滚、页表中间帧回收）
   - `T-SKT-03/04/05`（socket 失败回滚、unix dup 侧表、smoltcp 上限）
   - `T-PIPE-02`、`T-TS-03`、`T-FS-04`、`T-DRV-01/02`
3. 每完成一批：更新 `docs/audits/resource-fix-queue.md` 与 `docs/audits/resource-issues.md`（标注「已收敛」），**不要**擅自 git commit，除非用户要求。

### 必读文件

| 用途 | 路径 |
|------|------|
| 任务定义与验收标准 | `docs/tasks/audit_resource_lifecycle.md` |
| 修复队列（主清单） | `docs/audits/resource-fix-queue.md` |
| 问题详情 | `docs/audits/resource-issues.md` |
| 资源清单 | `docs/audits/resource-inventory.md` |
| 单资源深度说明 | `docs/audits/resources/*.md` |
| 交叉参考（避免重复修） | `docs/audits/syscall-issues.md`、`docs/audits/lock-issues.md` |
| 项目约束 | `docs/prompts/general.md`、`docs/prompts/coding.md` |

### 关键源码入口（按 P1 任务）

- 内存：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`、`user_heap_mmap.rs`、`mm-api/.../address_space.rs`；`sys/brk.rs`、`sys/mmap.rs`
- 堆：`os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`、`os/src/main.rs`
- socket：`os/components/wateros-syscall/.../sys/socket.rs`、`dup.rs`、`fcntl.rs`；`unix_sock.rs`、`socket_fd.rs`；`driver-network/src/lib.rs`
- pipe：`os/components/wateros-ipc/ipc-pipe/.../kernel_pipe.rs`
- 任务限额：`os/components/wateros-task/task-impl/impl-core/src/process.rs`
- 驱动：`os/components/wateros-driver/driver-impl/impl-qemu-*`；`impl-stack` 帧分配器

### 约束

- 不可靠路径：**warn + 明确错误返回 + partial alloc 回滚**（见 `audit_resource_lifecycle.md`）
- 验证：`cd os && make rv_check`；需要行为回归时 `make rv_qemu_run`
- 与用户沟通使用**简体中文**
- 修改范围保持最小，匹配现有代码风格

### 开始前

先读 `docs/audits/resource-fix-queue.md`，用 `git diff` 确认哪些 P0 已落地，再从 **T-KH-01** 和 **P1** 第一项未勾选项动手。

---

## 已完成波次（速查）

| 波次 | 任务范围 | 状态 |
|------|---------|------|
| 第一波 | T-PF-01、T-TS-01/02、T-KH-02 | 已完成 |
| 第二波 | T-FD-01/02/03、T-PIPE-01、T-SKT-02 | 已完成 |
| 第三波 | T-PF-02/03、T-IPC-02 | 已完成 |
| 第四波 | T-IPC-01/03、T-SKT-01 | 已完成 |
| 第五波 | T-PC-01/02/03、T-FS-01/02/03 | 已完成 |

完整条目与验收标准以 [`../resource-fix-queue.md`](../resource-fix-queue.md) 为准。
