# LTP 自主迭代任务（AI 托管）

## 任务目标

在**用户主动中断之前**，由 Agent 自主、循环地推进 WaterOS RISC-V LTP 适配：分析日志 → 排序问题 → 修复内核/bringup → 跑测 → 验收 → 提交 → 导出 → 记录历史。

**约束（不可违反）**

- 只改内核与 bring-up；**不修改**根卷内 `ltp_testcode.sh` 及 LTP 测例本身。
- 计分以日志中 **`TPASS` 行**为准；**仅当 TPASS 严格递增时才允许 git commit**。
- `FAIL LTP CASE xxx : 0` 且 Summary `passed > 0` 表示用例实质通过、runner 按退出码标 FAIL（分析时区分 TBROK/TFAIL/PANIC，**但不替代 TPASS 递增的提交门槛**）。
- **`os/Cargo.toml`、`os/src/user_bringup_busybox.rs` 仅作本地跑测/调试，不得 commit**。
- 与用户沟通使用**简体中文**。
- 所有编译、QEMU、export 操作在 **`os/`** 目录下通过 Makefile 完成。

## 停止条件

- **唯一停止条件**：用户主动中断（如发送「停止」「暂停迭代」等）。
- 未收到中断指令时，**一轮结束后自动进入下一轮**（回到步骤 1）。
- 若连续两轮 TPASS 无提升且无可推进的 P0 问题，仍继续循环，但须在 `history.md` 中标注「停滞轮」，并缩小 `n` 或转向更深层的单点攻坚（不得自行宣布任务结束）。

## 上下文恢复（对话被总结 / 压缩后必须执行）

一旦对话经历上下文总结、会话重启或 Agent 对任务边界产生不确定，**在开始任何步骤之前**重新完整读入：

| 顺序 | 路径 | 用途 |
|------|------|------|
| 1 | **本文件** `docs/prompts/tasks/ltp_autonomous_iteration.md` | 迭代循环与验收、提交规范 |
| 2 | `docs/prompts/general.md` | 构建运行、工作目录、Must-run 原则 |
| 3 | `docs/prompts/structure.md` | 目录与组件边界 |
| 4 | `docs/prompts/coding.md` | 编码与 Makefile 约定 |
| 5 | `docs/prompts/debug_workflow.md` | 调试与日志分析 |
| 6 | `docs/prompts/tasks/run_testsuits_qemu.md` | LTP 阶段开关与判读标准 |
| 7 | `os/tem/history.md` | **上一轮基线**（TPASS、修改摘要） |

可选按需阅读：`docs/exports/` 下与当轮失败子系统相关的组件导出。

---

## 每一轮循环（13 步）

### 步骤 1 — 检查日志，确认可推进的问题

**输入日志（优先级从高到低）**

1. `os/tem/` 下**最新** `rv_ltp_*.log`（本轮或上轮产物）
2. 若无，则 `os/rv_ltp.log`
3. 参考 `os/tem/history.md` 最近一条的 TPASS 基线

**必须执行的日志采集**

```bash
cd os
# 统计（写入当轮分析笔记，不必提交）
grep -c TPASS tem/rv_ltp_*.log 2>/dev/null | tail -1
grep -c TBROK tem/rv_ltp_*.log 2>/dev/null | tail -1
grep -c TFAIL tem/rv_ltp_*.log 2>/dev/null | tail -1
grep -E 'getpwnam|Panicked|unsupported: unknown nr=|SIGSEGV' tem/rv_ltp_*.log | tail -30
```

**可推进问题**须同时满足：

- 根因在内核 / VFS / syscall / bring-up（非缺赛题脚本、非缺用户态用例文件）。
- 有明确日志证据（TBROK/TFAIL/PANIC 行、内核栈、重复失败用例名）。
- 非「整个子系统未实现」且短期无法收敛的大洞（如完整 cgroup/eBPF），可记为 P2+ 暂缓。

**不可推进（本轮跳过或仅记录）**

- 纯环境/赛题脚本缺失（如 `cgroup_lib.sh`）且本轮不打算实现整个子系统。
- 需改 `ltp_testcode.sh` 才能「通过」的项。
- 同一问题连续两轮尝试均未改善且无线索（标记停滞，换方向）。

---

### 步骤 2 — 自行评估重要性并排序

**优先级框架（从高到低）**

| 级别 | 典型特征 | 示例 |
|------|----------|------|
| **P0** | 阻塞大量用例；早期 TBROK；SIGSEGV/PANIC | `getpwnam(nobody)`、页缓存脏读、未实现 syscall 致 panic |
| **P1** | 单测例族大量 TPASS 已具备、差最后一环 | access 权限语义、AF_UNIX bind 清理、capget 版本/掩码 |
| **P2** | 孤立 TFAIL、边界 errno、已实现子系统的缺口 | bind03 单条、adjtimex stub |
| **P3** | TCONF、脚本缺依赖、未实现大特性 | cgroup、bpf、完整网络栈 |

同优先级内，按 **「预计 TPASS 增量 × 实现把握」** 排序。

---

### 步骤 3 — 选择前 n 个问题（n 由 AI 自行决定）

**n 的启发式（不得询问用户，自行判断）**

| 情形 | 建议 n |
|------|--------|
| P0 且根因未明 / 需跨多 crate 调查 | **1** |
| P0 已知模式，可批量修（同一 syscall、同一文件） | **2** |
| 仅 P1/P2 小修、彼此独立 | **2～3** |
| 上轮刚完成大改，需控制回归半径 | **1** |
| 临近提交前需压缩 diff | **1** |

**单轮约束**

- 总 diff 以「可审查、可回滚」为原则；避免一轮内同时动 mm + 网络 + 全新子系统。
- 若选 n>1，问题之间应**低耦合**；否则合并为 1 个问题进行。

---

### 步骤 4 — 定位代码，制定修复计划

对选中的每个问题：

1. 从日志定位**用例名**与**失败行**（如 `access01.c:275`）。
2. 在 `os/components/`、`os/src/` 搜索相关 syscall、VFS、bring-up。
3. 写出**简短计划**（根因假设、改动文件列表、风险点），再动手。

**高频入口**

| 领域 | 路径 |
|------|------|
| LTP 开关 | `os/src/user_bringup_busybox.rs` |
| 根卷布局 / passwd | `os/src/user_bringup_root_layout.rs` |
| bring-up 总线 | `os/src/user_bringup_bus.rs` |
| syscall 实现 | `os/components/wateros-syscall/syscall-impl/impl-kernel/` |
| VFS / 页缓存 | `os/components/wateros-vfs/` |
| 网络 socket | `os/components/wateros-syscall/.../unix_sock.rs`、`wateros-driver/.../network/` |

---

### 步骤 5 — 执行修复计划

- 遵循 **最小正确 diff**；匹配现有命名与风格。
- 改完后执行：`cd os && make kernel-rv`（必须 0 错误）。
- 禁止无关重构、禁止顺手改赛题镜像内脚本。

---

### 步骤 6 — 调整日志与 LTP 运行配置（仅本地调试用，禁止提交）

以下文件**仅用于当轮跑测与调试**，不得进入步骤 10 的 `git add`；验收前须恢复为仓库已提交版本。

**6a. `os/Cargo.toml`（可选，临时开日志）**

在对应 board feature（`qemu-riscv64-opensbi`）中按需**临时**启用，例如：

```toml
# "runtime/impl-trace",
# "runtime/impl-info",
```

**6b. `os/src/user_bringup_busybox.rs`（每轮跑 LTP 前调整）**

确保 **仅 LTP** 在跑（其余 `*_testcode.sh` 保持注释）：

```rust
const SCRIPT_PATHS : &[&str] = &[
    "/glibc/ltp_testcode.sh",
    // "/musl/ltp_testcode.sh",
];
```

glibc/musl 是否同时启用由 Agent 自行判断（默认先 glibc only 缩短迭代）。

**6c. 跑分前必须恢复（强制）**

在步骤 7 运行 **用于验收计分** 的那次 QEMU 之前，执行：

```bash
cd /home/zhitian/project/WaterOS_refactor
git checkout -- os/Cargo.toml os/src/user_bringup_busybox.rs
cd os && make kernel-rv
```

- 若调试阶段改过上述文件，**计分日志必须基于恢复后的内核**跑出，否则步骤 8 的 TPASS 无效。
- 允许工作区保留未提交的 busybox/Cargo 改动用于下一轮调试，但 **commit 时工作区中这两文件必须与 HEAD 一致**（`git diff os/Cargo.toml os/src/user_bringup_busybox.rs` 为空）。

---

### 步骤 7 — 运行 LTP 并保存日志

**运行前检查（必须）**

```bash
pgrep -a qemu-system-riscv64 && pkill -9 qemu-system-riscv64
# 若 sdcard 被写坏（如 /etc/passwd 大小为 0），执行：make flush_img
mkdir -p os/tem
```

**运行**

```bash
cd os
TS=$(date +%Y%m%d_%H%M%S)
# 建议加超时：LTP 在 cgroup 段可能挂死，900s 可接受
timeout 900 make rv_qemu_run > "tem/rv_ltp_${TS}.log" 2>&1 || true
```

- 日志路径固定：`os/tem/rv_ltp_{时间戳}.log`
- `make rv_qemu_run` 会先 `kernel-rv` 再 QEMU；**不要**裸跑 qemu。
- 超时退出码 124 视为「部分跑完」，仍进入步骤 8 做部分验收。

---

### 步骤 8 — 根据日志验收（严格判定）

**核心指标**

```bash
LOG=os/tem/rv_ltp_${TS}.log   # 替换为当轮实际文件
TPASS=$(grep -c TPASS "$LOG")
TBROK=$(grep -c TBROK "$LOG")
TFAIL=$(grep -c TFAIL "$LOG")
```

**基线**

- 从 `os/tem/history.md` **最近一条已成功提交**的记录读取上轮 `TPASS`。
- 若无成功提交记录，以 `history.md` 首条基线或首轮记为 `0`；**不得以「例外」降低标准**。

**唯一成功条件（进入步骤 9～13 的必要条件）**

```text
本轮 TPASS > 上轮基线 TPASS   （严格大于，相等不算成功）
```

- 无其他例外：不因「P0 用例族修通」「SIGSEGV 消失」「TBROK 减少」而提交；**只看 TPASS 是否严格递增**。
- 计分日志须来自步骤 6c 恢复 `Cargo.toml` / `user_bringup_busybox.rs` 之后构建的内核。

**失败（回到步骤 1，不提交）**

- `TPASS <=` 上轮基线（含相等、下降）。
- 编译失败未解决。
- 瞄准问题无改善，或引入明显回归。
- 验收日志是在未恢复调试配置文件的情况下跑出（无效，需重跑）。

**记录**

- 在当轮笔记与 `history.md` 中列出：本轮 TPASS、上轮基线、ΔTPASS、是否触发超时；失败轮标注「未提交」。

---

### 步骤 9 — 生成 commit 信息（成功时）

**格式（必须严格遵守，不得附加其他段落）**

单行中文，前缀**三选一**（或组合，与历史提交一致）：

```text
[fix] <一句话说明修了什么、为什么>
[feat] <一句话说明新增能力>
[modify] <一句话说明行为/配置调整>
```

**允许的组合形式（与仓库历史一致）**

```text
[fix|feat] <一句话>
```

**禁止**

- 多行 body、Footer、`type(scope):` 式 Conventional Commits
- 英文 subject、issue 号、Co-authored-by
- 任何超出上述一行的内容

**示例**

```text
[fix] 修复 /etc/passwd 覆盖落盘为 0 字节导致 getpwnam(nobody) 失败，LTP 前 refresh 账户文件
```

---

### 步骤 10 — 在 `WaterOS_refactor` 仓库提交

**仅当步骤 8 验收成功（TPASS 严格递增）时执行。**

```bash
cd /home/zhitian/project/WaterOS_refactor
# 再次确认调试文件未纳入提交
git checkout -- os/Cargo.toml os/src/user_bringup_busybox.rs
git status
git diff
```

**禁止 `git add` 的文件（即使被改过）**

- `os/Cargo.toml`
- `os/src/user_bringup_busybox.rs`
- `os/tem/**`、`os/tem/*.log`
- `os/score.txt`、`os/*输出*.txt`、`qemu.log` 等临时产物

**只允许 add 本轮内核/bring-up 修复相关文件**（`os/components/**`、`os/src/**` 等，且排除上述禁止项）。

```bash
git add <files...>
git commit -m "$(cat <<'EOF'
[fix] 此处填步骤 9 生成的单行信息
EOF
)"
git status
```

- **禁止** `git push`（除非用户另行要求）。
- **禁止** 修改 `git config`。
- pre-commit 失败时：**不得** `git commit --amend` 旧提交；修问题后**新提交**。

---

### 步骤 11 — 导出到 GitLab 工作区

```bash
cd /home/zhitian/project/WaterOS_refactor/os
make export
```

将 `os/` 下 git 追踪文件同步到 `~/project/WaterOS_gitlab/os/`。

---

### 步骤 12 — 在 `WaterOS_gitlab` 做相同提交

`make export` **只拷贝文件，不产生 git 提交**。必须手动：

```bash
cd ~/project/WaterOS_gitlab
git status
git add <与步骤 10 相同的相对路径文件>
git commit -m "$(cat <<'EOF'
与步骤 9 完全相同的单行 commit 信息
EOF
)"
```

- 提交信息必须与 `WaterOS_refactor` **逐字相同**。
- 禁止 push，除非用户要求。

---

### 步骤 13 — 追加 `os/tem/history.md` 报告

在文件**末尾**追加（勿覆盖历史），格式：

```markdown
## {YYYY-MM-DD HH:MM:SS} 第{N}轮

### 目标
- 本轮选择的 n 个问题：…

### 修改
| 文件 | 改动摘要 |
|------|----------|
| `path/to/file` | … |

### 结果
- 日志：`os/tem/rv_ltp_{时间戳}.log`
- TPASS：{本轮}（上轮 {基线}，Δ{差值}）
- TBROK / TFAIL：…
- 重点用例：…
- 验收：成功 / 失败（失败则不计 commit，下轮继续）

### Commit
`[fix] …`（成功时填写，失败写「未提交」）

---
```

轮次 `{N}` 为文件中已有 `##` 标题数 + 1。

---

## 关键文件速查

| 路径 | 用途 |
|------|------|
| `os/tem/history.md` | 迭代历史与 TPASS 基线 |
| `os/tem/rv_ltp_*.log` | 当轮 QEMU 日志（不提交 git） |
| `os/Cargo.toml` | 日志 feature（**仅本地调试，禁止 commit**） |
| `os/src/user_bringup_busybox.rs` | LTP 脚本开关（**仅本地调试，禁止 commit**） |
| `os/Makefile` | `kernel-rv`、`rv_qemu_run`、`export` |
| `~/project/WaterOS_gitlab` | 比赛在线仓库本地镜像 |

---

## LTP 判读备忘

- **TPASS 行**：OJ 计分依据。
- **TBROK**：环境/前置失败（如 `getpwnam`、`tst_brk`）。
- **TFAIL**：断言失败，常比 TBROK 更接近「差一项语义」。
- **FAIL LTP CASE x : 0** + Summary `passed > 0`：用例可能已通过，优先消 TBROK/TFAIL 而非退出码。
- **卡点**：`cgroup_fj_proc` 附近 QEMU 挂死较常见；超时后仍可用已产生 TPASS 计分。
- **磁盘**：若怀疑 ext4 被写坏，`cd os && make flush_img` 后重跑。

---

## Agent 自检清单（每轮结束前）

- [ ] 验收日志在 `git checkout -- os/Cargo.toml os/src/user_bringup_busybox.rs` 之后跑出
- [ ] 已读 `history.md` 并成功提交轮次的 TPASS 基线
- [ ] **本轮 TPASS 严格大于上轮基线**（否则不得 commit）
- [ ] `make kernel-rv` 通过
- [ ] 日志已写入 `os/tem/rv_ltp_{时间戳}.log`
- [ ] commit 为**单行** `[fix]`/`[feat]`/`[modify]` 格式
- [ ] **未** add/commit `Cargo.toml`、`user_bringup_busybox.rs`、`tem/*.log` 等
- [ ] `WaterOS_refactor` 与 `WaterOS_gitlab` 提交信息一致
- [ ] `history.md` 已追加当轮报告
- [ ] 上下文被总结后已重新读入本文件与 `docs/prompts/`

---

## 改良说明（相对用户原始 13 步的增强）

以下已并入上文：

1. **严格验收**：仅当 `TPASS` **严格大于**上轮已成功提交基线时才可 commit；无例外。
2. **调试文件隔离**：`Cargo.toml`、`user_bringup_busybox.rs` 仅工作区调试，跑分前恢复，**永不提交**。
3. **QEMU 前置**：杀残留进程、镜像锁、`timeout 900` 防 cgroup 无限挂死。
4. **n 的启发式**：减少一轮改太多导致回归难查。
5. **提交白名单**：明确禁止提交日志与临时输出。
6. **GitLab 两步**：`export` ≠ `commit`，步骤 12 单独说明。
7. **上下文恢复清单**：总结后强制重读 prompts + 本任务 + history。
8. **flush_img 触发条件**：passwd 等根卷异常时恢复镜像。

可选后续增强（**未写入强制步骤**）：

- `os/tem/state.json` 机器可读基线。
- 每 5 轮 `make la_qemu_run` LoongArch 抽检。
- `git diff --stat` 写入 history。
