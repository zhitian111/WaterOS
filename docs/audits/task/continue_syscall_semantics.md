# 系统调用语义审计 — 新对话续作提示词

> 用途：在新 Agent 对话中复制下方「提示词正文」块，继续 syscall 审计收尾与实测修复。  
> 关联任务：`docs/tasks/audit_syscall_semantics.md`

---

## 提示词正文（复制即用）

你正在 **WaterOS_refactor** 仓库继续 **系统调用语义审计的收尾与实测修复**。

### 背景（已完成）

syscall 审计已多轮收敛（`docs/audits/syscall-issues.md` §7.1–§7.7），`make rv_check` / `make la_check` 已通过。锁审计 P0 亦已大面积修复（见 `docs/audits/lock-issues.md` §2）。**除非用户明确要求，不要动 LTP 旁路（P0-16）**。

### 你要做的事（按优先级）

1. **跑测例、对照日志，修回归**
   - 用 `os/rv_local_run.log`、`os/la_ltp.log` 定位仍卡死/失败项
   - 失败栈指向 syscall 则小步收敛；指向锁/调度则对照锁审计

2. **实现 §8「已知限制」中可独立推进的项**（选 1–2 项做小 PR，不要一次全做）：
   - `VfsMetadata` 增加 owner → `faccessat` 真实 owner 校验（VFS-P1-04）
   - `utimensat` 持久化到 ext4（VFS-P1-07）
   - `statfs` 接真实块统计（VFS-P1-03）
   - `wait4` 真实 `rusage`（PROC-P1-01）
   - cred 权限模型 / `shmctl` 扩展（MM-P1-04/05）

3. **锁审计暂缓项**（需设计评审，勿草率改）：
   - **NET-01**：`NETWORK_STACK` 全局 Mutex 拆分
   - **F-2**：futex `requeue_to` 持锁跨调度 → 对齐 `wake` 释锁模式

4. **仅当用户明确要求时**：P0-16 LTP `ltp_cgroup_helper.rs` fast-exit → `#[cfg(feature = "ltp-compat")]` 门控

5. **每轮代码后**：更新 `docs/audits/syscall-issues.md`、`docs/exports/features/wateros-syscall.md`；大项写入 `docs/roadmap/todolist.md`

### 必须先读的文件

| 文件 | 用途 |
|------|------|
| `docs/tasks/audit_syscall_semantics.md` | 任务目标与交付规范 |
| `docs/audits/syscall-issues.md` | **§7 已收敛** + **§8 已知限制**（工作清单） |
| `docs/audits/syscall-coverage.md` | 覆盖范围 |
| `docs/audits/syscall/*.md` | 5 组分组审计细节 |
| `docs/exports/features/wateros-syscall.md` | 能力快照（改代码后同步） |
| `docs/roadmap/todolist.md` | 路线图待办 |
| `docs/audits/lock-issues.md` | 锁问题；**§9 暂缓项** |
| `docs/audits/lock-inventory.md` | 锁清单与锁序 |

### 实现入口（按失败栈跳转）

| 目录/文件 | 用途 |
|-----------|------|
| `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs` | syscall 分发表 |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/` | 各 `sys_*` 实现 |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ltp_cgroup_helper.rs` | **暂缓**，勿默认修改 |
| `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs` | P0-14 flush 已修 |
| `os/components/wateros-ipc/ipc-futex/` | futex wake/requeue |
| `os/components/wateros-driver/driver-network/` | NET-01 相关 |
| `os/rv_local_run.log`、`os/la_ltp.log` | 实测失败证据 |

### 约束

- 收敛风格：**不支持 → `warn!`/`trace!` + 明确 errno**；禁止 panic 作为用户可见路径
- **不要**改 LTP fast-exit，除非用户明确说可以动
- 改完在 `os/` 下跑 `make rv_check la_check`
- 回复用简体中文

---

## 维护说明

- 每完成一轮显著收敛后，更新本文「背景」段与 `syscall-issues.md` §7/§8 保持一致。
- 若工作重心从 syscall 转向锁/资源审计，可另建 `docs/audits/task/continue_*.md` 平行文件。
