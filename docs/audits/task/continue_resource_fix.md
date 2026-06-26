# 资源生命周期修复 — 新对话续接提示词

> 用途：复制下方「Agent 提示词」整段到新对话，让下一个 Agent 从当前进度继续推进代码修复。  
> 最后更新：2026-06-26

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
| 第六波 | LTP `acct02`/shell 环境推进 | 已完成：`acct02` TPASS，已补 `testcases/bin/lib` PATH 与常用 busybox applet |
| 第七波 | LTP glibc A/B 段推进 | 进行中：`ar01.sh` 稳定 TCONF，`asapi_01` 从 TBROK 推进到 raw IPv6 TCONF，`arping`/`bbr` 推进到 veth/modules TCONF |

完整条目与验收标准以 [`../resource-fix-queue.md`](../resource-fix-queue.md) 为准。

最近实测日志：

- `os/tem/rv_ltp_20260625_203008.log`（240s timeout，已跑到 `bind05`）
- `os/tem/rv_ltp_20260625_203550.log`（180s timeout，已跑到 `ar01.sh`）
- `os/tem/rv_ltp_20260626_185832.log`（300s timeout，已跑到 `bind04`）
- `os/tem/rv_ltp_20260626_200239_close_range.log`（420s timeout，重新从 A 段跑到 `busy_poll01.sh`，未覆盖到 `close_range02`）
- `os/run.log`（当前工作树配置下已跑到 `cn_pec.sh`；`close_range02` 已 9 TPASS / 0 fail，尾部仍受 `clone303` cgroup 残留输出影响）

关键变化：

- `acct02`: `TPASS: acct() wrote correct file contents!`
- `add_ipv6addr`: 从 `check_envval: not found` 推进到缺少 `awk/cut`，补 applet 后继续推进到缺少 `locale`
- `ar01.sh`: 从 `grep: not found` 推进到缺少 `ar`；不要把 busybox 硬链接命名为 `ar`，当前 busybox 不含该 applet，误暴露会进入长耗时命令测试
- `arping01.sh`/`bbr*.sh`: `id`/`whoami`/`groups` 缺口已补，当前稳定 TCONF 于 veth driver/modules 缺失
- `asapi_01`: 已补 IPv4 `bind()` 接受 `AF_UNSPEC` 兼容语义，原 `sock_ntop: unknown AF_xxx` TBROK 消失；当前 raw IPv6 socket 返回 `EAFNOSUPPORT` 后 TCONF
- `ask_password.sh`/`assign_password.sh`: `TCSETS`/`TCSETSW`/`TCSETSF` 兼容 no-op 后 ioctl 0x5402 warning 消失；仍因无真实 MMC 密码环境逻辑失败
- `locale`/`ar`/`rsh`: 当前 busybox 不支持，启动时会主动清理 `/glibc`、`/musl`、`/bin`、`/usr/bin` 下的残留硬链接，避免把 TCONF 变成 applet TBROK 或长耗时挂起
- 当前 `os/run.log` 尾部：
  - `clone303`: cgroup helper 缺少 `cgroup.procs` 后重复 TBROK，最终用户态 `Segmentation fault`；末尾 `cn_pec.sh` 打出的 `clone303.c:45 ... ETIMEDOUT` 属于该残留/延迟输出，不是 `cn_pec.sh` 自身的首要缺口
  - `close_range02`: 原因是 nr=436 未分发导致 `ENOSYS`，并且测试里的 `clone(CLONE_FILES)` 被 fork 路径拒绝为 `EINVAL`
  - 已实现 `close_range(first,last,0)` 与 `CLOSE_RANGE_CLOEXEC`，未打开 fd 按 Linux 语义忽略，`first > last`/未知 flag 返回 `EINVAL`；`CLOSE_RANGE_UNSHARE` 暂按未建模语义拒绝
  - 已补 fork 路径 `CLONE_FILES`/`CLONE_FS` 继承语义：新地址空间进程可共享 fd 表/socket fd 表或 cwd
  - 已补 cgroup tmpfs 伪层级：cgroup v1/v2 子目录创建时自动生成 `cgroup.procs` 等控制文件，`rmdir` 允许只含控制文件的 cgroup 目录删除
  - 已补 `clone3(CLONE_INTO_CGROUP)` 兼容 no-op：校验 `cgroup` fd 是目录，剥离该扩展 flag 后走普通 clone；暂不建模真实 cgroup membership
