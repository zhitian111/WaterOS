---
handoff_schema: "codex-task-handoff/v1"
handoff_id: "HOF-20260813-loongarch-race-fix"
task_id: "loongarch-race-fix"
task_title: "LoongArch64 buildstorm 竞态定位、修复与稳定性验证"
task_status: "IN_PROGRESS"
handoff_status: "PARTIAL"
handoff_reason: "用户要求将当前 LoongArch 竞态修复上下文导出到 main，供新对话继续稳定性验证和剩余停顿调查。"
created_at: "2026-08-13T01:09:34+08:00"
updated_at: "2026-08-13T01:09:34+08:00"
freshness_deadline: "2026-08-14T01:09:34+08:00 — Git、进程、磁盘镜像与测试状态在此后必须重验"
created_by: "Codex/GPT-5/root-agent"
source_chat_id: "UNKNOWN — 当前运行时未提供 thread ID"
source_chat_title: "WaterOS performance and LoongArch buildstorm race investigation"
target_chat_id: "UNKNOWN until imported"
repository_name: "WaterOS_refactor"
repository_root: "/home/zhitian/project/WaterOS_refactor"
workspace_path: "/home/zhitian/project/WaterOS_refactor"
workspace_kind: "local"
git_branch: "main"
git_head: "c413c4b97e36f91b1b4ba960209d00509f45ebd5"
git_base_branch: "github/main"
git_base_commit: "9447d5d51c5c366667fbe91e930bc586b0b870b4"
working_tree_state: "dirty"
primary_platform: "x86_64 host; LoongArch64/QEMU 9.2.1 target; RISC-V64 regression target"
confidentiality: "internal"
---

# 0. 交接契约与阅读规则

## 0.1 接管者必须遵守的事实优先级 [REQUIRED]

出现冲突时依次服从：接管后的用户新指令；当前仓库/命令事实；适用的
`AGENTS.md` 和项目规范；本交接仍新鲜的 `[VERIFIED]`；`[OBSERVED]`、
`[INFERRED]`、`[HYPOTHESIS]`；旧对话叙述。发现差异时必须先写差异报告，
不能用 reset/clean/checkout 等手段强行让现场匹配本文件。

## 0.2 状态标签 [REQUIRED]

- `[USER-REQ]` / `[USER-CORRECTION]`：用户要求或纠正。
- `[VERIFIED]`：由当前代码、Git 或已保存测试证据确认。
- `[OBSERVED]`：直接观察但原因未完全证明。
- `[INFERRED]` / `[HYPOTHESIS]`：证据支持的推断 / 待验证解释。
- `[DECISION]`：已采用且应保持的决定。
- `[TODO]` / `[BLOCKED]`：未完成 / 被当前问题阻塞。
- `[STALE]` / `[UNKNOWN]` / `[N/A]`：需重验 / 未知 / 不适用。

## 0.3 证据书写格式 [REQUIRED]

每个新结论至少记录标签、命令或文件、branch/HEAD/dirty、ISO 时间、exit code
或关键输出。运行证据必须区分其对应的 commit；不得因后继 commit 包含旧修改就
把旧测试冒充为当前 HEAD 的完整验证。

## 0.4 本交接的边界

- 本文件包含所有旧对话信息：`NO` — 仅完整覆盖 LoongArch buildstorm 竞态、
  关联 ABI/内存修复、当前停顿和验证约束；长期性能优化的全部分支历史不在范围内。
- 根据当前仓库重新核验：`YES` — Git、代码、关键 commits、日志、进程、mount、
  QEMU 版本和输入 hash 已重新检查。
- 未能读取的来源：线上评测机内部状态；旧对话的内部 thread ID；被终止 VM 的
  内存现场（只保留当时抄录的页表 walk）。
- 未能运行的命令：本次导出没有重新执行 8–10 分钟级全量 buildstorm；没有重建
  两架构内核，因为用户要求的是导出且已有相应构建记录。
- 安全省略：评测脚本正文不写入交接；14–15 GiB 镜像和 130+ MiB 内核不复制。

# 1. 执行摘要

## 1.1 一句话状态 [REQUIRED]

[VERIFIED] `main` 已包含 LoongArch ASID 首次上 CPU 刷新 (`9bc17023`) 和并发
COW 二次 fault 重试 (`705950b4`)；后者由“PME 时真实 PTE 已 W+D+U”的现场直接
支撑。[PARTIAL] 修复后一次测试未再出现 SIGSEGV，但停在 `hashbrown` 且没有完成，
因此下一步是用当前 `main` 和原始恢复脚本做三轮干净 LoongArch 全量验证，并把
剩余停顿与已修 COW race 分开调查。

## 1.2 任务目标 [REQUIRED]

让 `main` 的最佳 final LoongArch 内核在 QEMU 9.2.1、12 vCPU、线上等价 buildstorm
脚本下稳定完成编译、嵌套 QEMU 启动和评分输出；同时不回归已稳定的 RISC-V 行为，
不泄露或修改线上无法改变的评测脚本语义。

## 1.3 当前完成度 [REQUIRED]

- 总体估计：`70%`。故障现场和 COW race 修复已完成，构建通过；尚缺当前 HEAD 的
  LoongArch 3/3 全量通过以及 hashbrown/UEFI 停顿的根因闭环。
- 已完成：Linux signal-frame ABI、LoongArch DTB 完整内存范围、trap/signal padding、
  ASID 首次 CPU 使用刷新、并发 COW fault 重试、移除评测脚本正文打印，均已进入 main。
- 部分完成：当前 COW 修复的运行验证；LoongArch 稳定性验证。
- 未开始：如果停顿仍复现，针对 futex/wait 生命周期或 LoongArch IPI/TLB shootdown
  的最小插桩实验。
- 当前阻塞：运行结果不稳定；最新一次修复后样本停在 `Compiling hashbrown`，所有
  12 个 vCPU 进入 idle，未产生最终结果。

## 1.4 接管后第一项操作 [REQUIRED]

```bash
cd /home/zhitian/project/WaterOS_refactor && \
git status --porcelain=v2 --branch && git rev-parse HEAD && \
ps -eo pid,stat,etime,args | rg 'qemu-system-(loongarch64|riscv64)|gdb.*kernel-la' || true
```

目的：确认仍为 `main@c413c4b9`、只存在本交接和 13 个受保护 untracked，且无遗留 VM。

- 若一致：只读核对第 13 节输入后，从 `NEXT-001` 继续。
- 若 HEAD 前移但相关 commits 仍为祖先：记录差异，重建 `kernel-la` 再测试。
- 若有未知 tracked 修改、正在运行的 QEMU/GDB 或镜像 mount：停止写操作，先保护现场。

## 1.5 最大风险

把“COW SIGSEGV 机制已定位并修复”错误外推为“LoongArch 全部竞态已修复”。当前
`hashbrown`/UEFI 停顿仍可复现且可能是独立的 missed wake、IPI shootdown 或调度问题。

## 1.6 当前是否需要用户决定

- `None`。用户已经授权继续定位并要求每架构最终共 3 轮。
- 可安全继续：只读核验、构建 final 内核、在干净镜像副本上运行原始恢复脚本、
  收集停顿现场。不要修改线上脚本逻辑来“通过”测试。

# 2. 任务来源与对话上下文

## 2.1 来源对话

| 字段 | 内容 |
|---|---|
| 对话/线程 ID | UNKNOWN — 产品未暴露 |
| 对话标题 | WaterOS 性能优化与 LoongArch/RISC-V buildstorm 修复 |
| 起始时间 | UNKNOWN — 长对话跨多次上下文压缩 |
| 最后更新时间 | 2026-08-13T01:09:34+08:00 |
| 与其他对话的关系 | 原始持续对话，期间使用过多个临时 worktree |
| 旧对话是否仍可访问 | 当前会话可通过压缩摘要访问关键内容；新对话不可依赖它 |

## 2.2 用户意图摘要 [REQUIRED]

优先级曾从性能优化切换到“让线上两个架构都能正常出分”。RISC-V 已基本确认稳定，
LoongArch 出现编译中随机停顿和一次 `SIGSEGV signal not delivered`。用户要求从代码
和可复现现场找竞态、合并有效修复到 main、用线上 QEMU 9.2.1 等价环境和原始脚本
验证；最终每个架构三轮，并保持 `make all` 只交付最佳 `kernel-rv`/`kernel-la`。

## 2.3 用户要求与纠正时间线 [REQUIRED]

| ID | 时间/回合 | 类型 | 规范化要求 | 来源摘要 | 状态 |
|---|---|---|---|---|---|
| UREQ-001 | 早期 | USER-REQ | 线上脚本下 RV/LA 必须跑完并产生正常分数，优先于性能优化 | “这两个问题远比性能优化重要” | open：RV 稳定，LA 未闭环 |
| UCOR-001 | 早期 | USER-CORRECTION | 以测试镜像/线上 QEMU 9.2.1 为准，不以 QEMU 11 为准 | 用户给出两架构 9.2.1 版本和完整命令 | applied |
| UREQ-002 | 中期 | USER-REQ | 使用 `~/Downloads/buildstorm_testcode.recovered.sh` 和重新解压的干净镜像 | “原本的脚本…重新解压一份” | applied；本地测试镜像为替换脚本副本 |
| UCOR-002 | 中期 | USER-CORRECTION | 脚本修改只能辅助定位，不能作为线上修复 | “我们是改不了线上测试脚本的” | binding |
| UREQ-003 | 中期 | USER-REQ | LoongArch CPU/内存差异用条件编译和 DTB 正确处理，不能靠错误识别 | 关于 MAX_CPU 与设备树 memory 的纠正 | done for current known changes |
| UREQ-004 | 中期 | USER-REQ | `make all` 必须且只能生成当前 main 最佳的 `kernel-rv`、`kernel-la` | 多次明确“只能保留这两个” | active invariant |
| UREQ-005 | 后期 | USER-REQ | 修复后 RV、LA 各总共做 3 次全量验证 | “每个架构都总共做3次测试” | RV reported 3/3; LA open |
| UREQ-006 | 后期 | USER-REQ | 先收集 SIGSEGV 现场，再确认稳定/偶发；若 debug 扰动时序则回到代码链路 | 关于 SIGSEGV 和 hashbrown 的连续指令 | applied |
| UDEC-001 | 后期 | USER-DECISION | LoongArch ASID 首次使用修复在一次成功越过卡点后先合入 main | “先合并到main上吧” | done (`9bc17023`) |
| UREQ-007 | 后期 | USER-REQ | 删除打印评测脚本正文并提交 main，比赛方禁止逆向 | “删除掉我们打印…因为比赛方不允许” | done (`1f56057a`) |
| UREQ-008 | 当前 | USER-REQ | 导出 LoongArch 竞态 handoff 到当前 main | 当前消息 | done by this directory；未 commit |

## 2.4 用户偏好与协作方式

- 输出形式：结论优先，保留精确命令和结果文件；避免倾倒无意义编译日志。
- 修改策略：从 Linux 机制和本项目缺口出发，最小、架构特定修复；有效后进入 main。
- 验证偏好：A/B 一次明确有效即可用于性能实验；本竞态则明确要求最终 3 轮/架构。
- 日志约束：通常只看结果文件；命令失败或需要具体根因时再读日志。
- 禁止：污染 pub 镜像、通过更改线上脚本语义绕过问题、打印/逆向评测脚本正文、
  同时运行会抢 CPU 的多余 QEMU。

## 2.5 仍存在的歧义

| ID | 歧义 | 证据 | 临时解释 | 影响 | 解决方式 |
|---|---|---|---|---|---|
| AMB-001 | 修复后 hashbrown 停顿是否与原 SIGSEGV 同源 | COW 修复后无 SIGSEGV但所有 CPU idle | 视为独立未解决问题 | 若混为一谈会误判完成 | 三轮测试+停顿现场对比 |
| AMB-002 | 本地 36G guest 在 30GiB host 上是否影响稳定性 | QEMU RSS约2.7G、host available约17G；停顿时 CPU低 | 不是已观察 guest 调度停顿的充分解释 | 可能增加噪声 | 记录 host pressure；线上 36G 再确认 |

# 3. 目标、范围与完成定义

## 3.1 最终目标 [REQUIRED]

当前 main 的 LoongArch final 内核在与线上一致的 9.2.1/12-vCPU/virtio-blk-pci
环境中稳定完成原始 buildstorm，嵌套 QEMU 输出 `Hello, world!`，评分脚本输出
`BUILDSTORM_RESULT ... status=OK ... run=OK`；RISC-V 行为不回归。

## 3.2 本次工作范围 [REQUIRED]

### In scope

- LoongArch ASID/TLB、COW page fault、signal/trap、调度/futex/IPI 相关竞态。
- 原始恢复脚本、干净镜像副本、QEMU 9.2.1 的运行验证。
- `main` 上 LoongArch 特定修复与 RV 回归检查。

### Out of scope

- 通过更改评测脚本绕过 boot/run 判定。
- 继续 page cache、metadata cache、per-CPU heap、VirtIO IRQ 性能开发。
- 修改或发布线上评测资源。

### Deferred

- 全局性能优化直到正确性稳定。
- 普通块缓存扩容/统一 page cache、IRQ-framework 异步 VirtIO 实验。

## 3.3 非目标与禁止性要求 [REQUIRED]

- 不需要实现：新的评分脚本或假 `Hello, world!` 输出。
- 不允许改变：恢复脚本的编译、计时、嵌套 QEMU 和最终判定语义。
- 不允许引入：脚本正文打印、线上不可用的 QEMU 11 专属行为。
- 必须保持兼容：QEMU 9.2.1；LoongArch glibc/Linux signal ABI；RISC-V final。
- 用户明确否决：用已污染 pub 镜像得结论；用脚本 workaround 代替内核修复。

## 3.4 交付物清单 [REQUIRED]

| ID | 交付物 | 路径/位置 | 状态 | 验收方式 |
|---|---|---|---|---|
| DEL-001 | ASID 首次 CPU 使用刷新 | `user_aspace.rs::mark_active` | DONE | commit + 一次 full success |
| DEL-002 | 并发 COW 二次 fault 重试 | `pagetable.rs::handle_cow_fault_no_flush` | PARTIAL | build 通过；尚缺 3/3 runtime |
| DEL-003 | 禁止脚本正文打印 | `user_bringup_busybox.rs` | DONE | commit；关键字符串不存在 |
| DEL-004 | LoongArch 三轮稳定验证 | handoff `logs/` 后续追加 | TODO | 三次完整 status=OK/run=OK |
| DEL-005 | 本可验证 handoff | `docs/agent/handoffs/loongarch-race-fix/` | DONE | 第 24 节审计 |

## 3.5 完成标准 / Definition of Done [REQUIRED]

- [ ] 功能：LoongArch 3/3 原始脚本完整结束，无 SIGSEGV、hashbrown/UEFI 停顿。
- [ ] 兼容：嵌套 QEMU 9.2.1 检测到 `Hello, world!`，`run=OK`。
- [x] 构建：相关修复曾通过 `make kernel-la`；最终 HEAD 仍需重建确认。
- [ ] 自动/手工验证：LA 3/3；若后续改公共代码，RV 3/3 重新跑。
- [ ] 性能/资源：本地 `elapsed_s` 无灾难性回归；正确性优先。
- [x] 文档：本 handoff 记录证据、风险和下一步。
- [x] 清理：main 无 fault probe 调试代码，无脚本正文打印；无活动 QEMU/GDB。
- [ ] Git：交接是否提交由用户/接管者决定；不得触碰既有 untracked。
- [x] 已知限制明确记录。

## 3.6 停止条件

- 发现测试将覆盖唯一 pub/决赛镜像，而没有明确的可重建源。
- 实际 HEAD/工作树与交接差异包含未知 tracked 修改。
- 需要修改恢复脚本逻辑才能“通过”。
- 需要删除/覆盖 13 个既有 untracked、其他 dirty worktree 或 stash。

# 4. 需求追踪矩阵

| ID | 类型 | 要求 | 来源 | 优先级 | 状态 | 实现位置 | 验证证据 | 缺口 |
|---|---|---|---|---|---|---|---|---|
| REQ-F-001 | functional | LA 全量 buildstorm 稳定完成 | UREQ-001/005 | MUST | PARTIAL | MM/trap/scheduler | full-1 成功；后续停顿 | LA 3/3 |
| REQ-F-002 | functional | 已并发解决的 COW fault 不误杀 | UREQ-006 | MUST | PARTIAL | `pagetable.rs:992` | fault probe+commit | current 3/3 |
| REQ-COMP-001 | compatibility | QEMU 9.2.1 嵌套 boot 可用 | UCOR-001 | MUST | PARTIAL | HWCAP+kernel | full-1 `run=OK` | current HEAD 重验 |
| REQ-COMP-002 | compatibility | 不回归 RISC-V | UREQ-005 | MUST | PARTIAL | LoongArch cfg-specific files | earlier RV 3/3 (conversation) | 公共改动后重验 |
| REQ-SAFE-001 | safety | 不输出评测脚本正文 | UREQ-007 | MUST | DONE | `user_bringup_busybox.rs` | commit `1f56057a`; rg empty | none |
| REQ-BUILD-001 | build | `make all` 只交付两个最佳 final 内核 | UREQ-004 | MUST | DONE/code | `os/Makefile:517-541` | code check | 最终构建可重验 |
| REQ-PERF-001 | performance | 正确性完成后继续性能优化 | 长期目标 | SHOULD | DEFERRED | N/A | N/A | 当前优先级更低 |
| REQ-DOC-001 | documentation | 可验证交接进入当前 main 工作树 | UREQ-008 | MUST | DONE | 本目录 | audit | 未 commit |
| REQ-NOT-001 | prohibition | 不靠改线上脚本修内核 | UCOR-002 | MUST NOT | ACTIVE | 测试流程 | 本文约束 | 持续遵守 |

# 5. 已加载的持久指令与规范

## 5.1 指令文件清单 [REQUIRED]

| 顺序 | 路径 | 适用范围 | 关键规则 | SHA-256/状态 |
|---:|---|---|---|---|
| 1 | `AGENTS.md` | 全仓库 | CodeGraph 优先；Rust/QEMU 构建规范；保护 vendor/用户改动 | `e82fce0d…` |
| 2 | `docs/agents/tasks/CODEX_HANDOFF_EXPORT_PROMPT.md` | 本导出 | 精确 Git/运行状态、证据包、不得泄密/破坏 | `52f1f6f6…` |
| 3 | `docs/agents/tasks/CODEX_HANDOFF_TEMPLATE.md` | 本文结构 | 所有 REQUIRED 填写；N/A/UNKNOWN 不省略 | `062a14a0…` |
| 4 | `docs/agents/skills/CODEX_VERIFIED_HANDOFF_SKILL.md` | 交接流程 | EXPORT 后重读审计 | `d35c4523…` |
| 5 | `os/AGENTS.md`, `os/AGENT.md` | 用户早先提及 | 当前仓库不存在 | MISSING；接管时重新确认 |

## 5.2 其他规范与文档

- `docs/tasks/perf/performance-optimization-handoff-20260811.md`：长期性能背景；不是本竞态
  handoff 的真值源。
- `os/Makefile:517-541`：`make all` final 构建与仅两个交付物检查。
- `.codegraph/` 存在；本次用 CodeGraph 核验 `mark_active`、COW call path。

## 5.3 指令冲突及处理

无已知冲突。日志保留要求与“禁止逆向”通过删去旧日志中的逐字符脚本正文解决；
故障行、开始/结束标记和测试结果保留。

# 6. 项目与仓库地图

## 6.1 仓库集合 [REQUIRED]

| 仓库/worktree | branch@HEAD | 状态 | 关系 |
|---|---|---|---|
| `/home/zhitian/project/WaterOS_refactor` | `main@c413c4b9` | dirty：本 handoff+13既有 untracked | 当前主仓库 |
| `WaterOS_loongson2k1000_port` | `feat/loongson2k1000-port@f3cebb77` | clean | 不触碰 |
| `WaterOS_real_hardware_ports` | `feat/real-hardware-common@2d5a68e2` | untracked `.codegraph/` | 不触碰 |
| `WaterOS_runtime_image_bringup` | `fix/runtime-image-bringup@7f3d646d` | clean | 不触碰 |
| `WaterOS_visionfive2_port` | `feat/visionfive2-port@70a8dead` | clean | 不触碰 |
| `/tmp/wateros-elf-cache-128m` | `perf/elf-cache-128m@d47367e2` | dirty report | 性能实验，受保护 |
| `/tmp/wateros-perf-tools` | detached `765e0257` | untracked `final_test_case` | 受保护 |

完整列表：`snapshots/git-worktrees.txt`。

## 6.2 目录与模块职责

| 路径 | 职责 |
|---|---|
| `os/components/wateros-mm/mm-impl/impl-loongarch64/` | LA 页表、ASID、用户地址空间、TLB shootdown |
| `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/` | trap/CSR/上下文实现 |
| `os/components/wateros-syscall/.../ipc/signal.rs` | Linux signal frame/return |
| `os/src/user_bringup_busybox.rs` | 镜像内测试命令 bringup；不得输出脚本正文 |
| `os/Makefile` | 两架构 final 内核构建与交付物约束 |

## 6.3 架构概览

Store PME → trap page-fault path → `wateros_mm::handle_cow_fault` → LA
`kernel_mm_impl::handle_cow_fault` → address-space lock → PTE COW 处理 → 若 handled，
本地按页 invalidation + 目标 aspace CPU shootdown → 重试用户 store。地址空间切换通过
PGDL+ASID；CPU 首次运行该 aspace 时清本地 TLB，避免 ASID 复用残留。

## 6.4 当前任务关键 symbol [REQUIRED]

| Symbol | 文件:行 | 作用/审查点 |
|---|---|---|
| `mark_active` | `user_aspace.rs:77` | 首次 CPU 使用 `tlb_cpus.fetch_or` 后 full local flush |
| `with_user_aspace_mut_and_page_flush` | `user_aspace.rs:240` | 锁内 mutation；handled 时 local page flush+remote shootdown |
| `handle_cow_page` | `pagetable.rs:953` | 真正 COW 复制/恢复写权限 |
| `handle_cow_fault_no_flush` | `pagetable.rs:992` | 并发 loser 识别 W+D+U leaf 并返回 handled |
| `handle_cow_fault` | `impl-loongarch64/src/lib.rs:175` | 外层把 bool 同时作为 handled/changed 触发 flush |

## 6.5 重要不变量

- ASID 0 仅内核；用户 aspace token 携带 PGDL+10-bit ASID。
- 真正只读、非用户、非 level-0 leaf 或 unmapped fault 仍必须返回 false/走 SIGSEGV。
- 同一 aspace PTE mutation 由 `MultiprocessorSafeCell` 串行化，但 CPU 可在拿锁前都
  基于旧 TLB 产生 fault。
- `make all` 结束时只能有 `kernel-la` 与 `kernel-rv` 两个交付物。

# 7. 环境快照

## 7.1 主机与工作区 [REQUIRED]

| 项 | 值 |
|---|---|
| Host | `Linux shy-archlinux 7.1.5-arch1-2 x86_64` |
| CWD/repo | `/home/zhitian/project/WaterOS_refactor` |
| 时间/时区 | `2026-08-13T01:09:34+08:00`, Asia/Shanghai |
| CPU/内存 | host 30 GiB；采样时 available 17 GiB，swap 2.4/8 GiB used |
| 磁盘 | `/` 245G，220G used，14G available，95%；`/tmp` tmpfs 16G，9.6G available |
| shell | zsh |

## 7.2 工具链与版本 [REQUIRED]

| 工具 | 版本/路径 |
|---|---|
| rustc | `1.96.0-nightly (3645249d7 2026-03-16)`, LLVM 22.1.0 |
| cargo | `1.96.0-nightly (cbb9bb8bd 2026-03-13)` |
| local QEMU LA | `/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64`, 9.2.1 |
| online QEMU | user observed both LA/RV `9.2.1` |
| linker/objcopy | target-specific rustup LLVM tools；精确版本未单独保存 |

## 7.3 依赖与锁定状态

- Rust workspace lock/config 使用仓库当前状态；本交接未变更依赖。
- `user` 是 Git submodule，当前 `2f470f95…`。
- Git LFS 无输出；不依赖 LFS。

## 7.4 相关环境变量

- 测试脚本内部设置 Rust/Cargo homes 和 offline 模式；为避免逆向限制不复制正文。
- QEMU loader 使用其自带 `ld-linux-loongarch-lp64d.so.1` 与 library path。
- 无凭据相关环境变量需要记录。

## 7.5 环境建立与恢复命令

```bash
cd /home/zhitian/project/WaterOS_refactor/os
make kernel-la
/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64 --version
```

干净镜像从 `~/Downloads/sdcard-la-pub.img.gz` 解压到新文件，之后只替换
`/glibc/buildstorm_testcode.sh` 为恢复脚本。不要覆盖唯一 pub 镜像。

## 7.6 已知环境差异

- 线上 LA：36G RAM/12 CPU；本地相同 guest 参数，但 host 物理内存仅 30GiB，QEMU
  按需 RSS 在已观察样本约 2.7GiB。
- 本地镜像名/内容来自 pub 压缩包加恢复脚本；决赛镜像可能有其他差异。
- `os/kernel-la` mtime 为 2026-08-11，不能作为当前 HEAD 构建产物使用；必须重建。

# 8. Git 与工作区精确快照

## 8.1 仓库身份 [REQUIRED]

| 字段 | 值 |
|---|---|
| branch/HEAD | `main` / `c413c4b97e36f91b1b4ba960209d00509f45ebd5` |
| upstream | `github/main` |
| ahead/behind | `+89/-4` |
| merge-base | `9447d5d51c5c366667fbe91e930bc586b0b870b4` |
| remotes | `github` SSH、`gitlab` HTTPS；URL 无嵌入凭据 |

## 8.2 `git status` [REQUIRED]

导出前快照在 `snapshots/git-status-porcelain-v2.txt`：tracked staged/unstaged 均空，
13 个既有 untracked。导出后新增本 handoff 目录，因此仍为 dirty。

## 8.3 最近提交

```text
c413c4b9 [merge] integrate GitLab README layout
705950b4 [fix] retry concurrently resolved LoongArch COW faults
1f56057a [fix] stop exposing competition script contents
acf60257 [merge] integrate documentation and tooling updates
9bc17023 [fix] flush reused LoongArch ASIDs on first CPU use
2cc13e52 [fix] initialize trap and signal ABI padding
2a7d2712 [fix] use full LoongArch DTB memory range
cc62fc75 [fix] match Linux signal frame ABI
```

完整 20 条在 `snapshots/git-log-20.txt`。

## 8.4 Staged 修改 [REQUIRED]

`None`。`snapshots/staged.patch` 为 0 bytes。

## 8.5 Unstaged 修改 [REQUIRED]

tracked unstaged 为 `None`。`snapshots/working-tree.patch` 为 0 bytes。handoff 本身未跟踪。

## 8.6 Untracked 文件 [REQUIRED]

- 当前任务新增：`docs/agent/handoffs/loongarch-race-fix/` 全目录。
- 导出前已有且所有权不明的 13 个文件：一个 `plic.rs`、六个 baseline/test kernel
  或 config、六个镜像脚本备份。精确路径/大小/mtime：
  `snapshots/preexisting-untracked-files.txt`；SHA-256：
  `snapshots/preexisting-untracked-sha256.txt`。
- 不得覆盖、删除或顺手提交这 13 个文件。

## 8.7 Required ignored 文件 [REQUIRED]

| 路径 | 作用 | 状态/重建 |
|---|---|---|
| `os/kernel-la`, `os/kernel-rv` | final 交付内核 | ignored；mtime 过旧，`make all` 或各 target 重建 |
| `os/sdcard-la-pub.img`, `os/sdcard-rv-pub.img` | pub 测试镜像，各 15,032,385,536 bytes | ignored；不要原地污染 |
| `os/sdcard-la-tlb-test.img` | LA 恢复脚本测试副本，15,032,385,536 bytes | ignored；脚本 inode size 7549 |
| `/tmp/kernel-la-fault-probe-1f56057a` | 只用于已结束 probe，138,853,032 bytes | 外部临时产物；可由旧 commit+probe重建 |

详见 `snapshots/required-artifact-stat.txt` 和 `external-input-sha256.txt`。

## 8.8 Worktree、stash、submodule、LFS

- 7 个 worktree，两个 `/tmp` worktree dirty；见 6.1 和 snapshot。
- 5 个历史 stash，均非本任务创建；不得 drop/pop 以“清理”。
- submodule `user@2f470f95…`；LFS 无记录。

## 8.9 Remote 信息（脱敏）

- `github`: `git@github.com:<owner>/WaterOS.git`
- `gitlab`: `https://gitlab.eduxiji.net/<team>/wateros.git`
- 本交接没有 fetch/push；ahead/behind 可能过期。

## 8.10 补丁与快照

- 修复 commits 完整 patch：`snapshots/relevant-commits.patch`, 6,686 bytes,
  SHA-256 `c852eaf0…`（已省略提交者邮箱元数据）。
- 导出前 status：`snapshots/git-status-porcelain-v2.txt`, SHA-256 `5049ae48…`。
- 进程/mount/loop 快照分别在 `processes.txt`、`findmnt.txt`、`losetup.txt`。

## 8.11 用户已有修改与所有权边界 [REQUIRED]

本 handoff 目录是当前导出产生。其余 13 个 untracked、5 个 stash、其他 worktree 的
dirty 内容均按用户/其他任务所有处理。本任务相关代码已经 commit，不存在未提交源码。

# 9. 文件与代码修改清单

## 9.1 已修改文件 [REQUIRED]

| 文件 | Git 状态/commit | Symbol | 前→后行为 | 完成度/证据 | 风险 |
|---|---|---|---|---|---|
| `.../impl-loongarch64/src/user_aspace.rs` | tracked, `9bc17023` | `mark_active` | 仅记录 CPU → CPU 首次运行 aspace 时 local full TLB flush | code verified；full-1成功 | full flush 成本；不能证明全部 hang |
| `.../impl-loongarch64/src/pagetable.rs` | tracked, `705950b4` | flags `dirty/user`, `handle_cow_fault_no_flush` | 非 COW 即 false → 若当前 leaf W+D+U 则 handled+flush retry | build + probe 支撑；runtime partial | 必须确保真 RO fault 不吞掉 |
| `os/src/user_bringup_busybox.rs` | tracked, `1f56057a` | 删除 `dump_script_body` 调用链 | 执行前逐字符打印 → 不打印正文 | rg 无关键 symbol | 无功能性脚本变更 |

## 9.2 新增文件

本 handoff 目录；不属于内核运行代码。

## 9.3 删除或重命名

`N/A` — 当前导出未删除/重命名源码；`1f56057a` 仅删除函数和调用。

## 9.4 未修改但必须先阅读的文件

- `impl-loongarch64/src/lib.rs::handle_cow_fault`：bool 同时决定 handled 与 flush。
- `impl-loongarch64/src/asid.rs` 和 paging/shootdown 实现：若剩余 hang 指向 TLB/IPI。
- futex registry、task wait/wake、LoongArch IPI 发送/接收：仅在复现停顿后按调用链阅读。
- `os/Makefile:517-541`：交付物不变量。

## 9.5 生成文件和构建产物

`kernel-la`, `kernel-rv`, `target/`, `*.img`, `/tmp/kernel-la-fault-probe-*` 均不可当源码。
当前 `os/kernel-la`/`kernel-rv` 早于相关修复，重测必须重建。

## 9.6 代码审查关注点

- COW loser 分支检查 `level==0 && leaf && W && D && PLV_USER`，而不是仅 W。
- outer wrapper 在返回 true 时一定执行 local page flush 和 peer shootdown。
- `mark_active` 的 bitset/Ordering 与 destroy/ASID reuse 生命周期。
- 不要把 RISC-V clone ABI “修成”通用参数顺序；此前已核对 LA glibc clone 汇编映射正确。

# 10. 当前实现状态与行为模型

## 10.1 已完成实现 [REQUIRED]

### IMPL-001：ASID 首次 CPU 使用防陈旧翻译

- 输入：aspace handle、CpuId。
- 状态：`tlb_cpus` bitset。
- 行为：`fetch_or` 返回值显示该 CPU 第一次见该 aspace 时，full local flush。
- 错误/边界：dropped handle 或 CPU id >=64 返回；LA 当前最多12核。
- 证据：`9bc17023`; `wateros-la-tlb-firstuse-full-1.log` 完整成功。

### IMPL-002：并发已解决 COW fault 重试

- CPU A/B 都基于旧 D=0/COW translation 触发 PME。
- A 拿锁，恢复/复制 PTE 为 W+D，flush；B 之后拿锁时 COW bit 已清。
- 旧实现 B 返回 false 并走 SIGSEGV；新实现核对真实 leaf W+D+U，返回 true，
  outer wrapper invalidates 并重试 store。
- 证据：fault probe 的 PME 与 W+D+U leaf 同时成立；`705950b4`。

### IMPL-003：合规移除脚本正文打印

`SCRIPT_BODY_FLAT` 代码路径已移除；交接日志副本也删去了旧逐字符正文。

## 10.2 部分实现

### IMPL-P-001：LoongArch buildstorm 稳定性

- 已消除一个可证明的 COW misclassification 机制。
- 修复后样本在 `hashbrown` 停住，未见 SIGSEGV，但不能证明信号问题完全消失。
- 未闭环：all-vCPU-idle 时是否存在 missed futex wake、丢失 runnable task、IPI/TLB
  shootdown wait 或进程生命周期问题。

## 10.3 尚未开始

- 针对最新 hashbrown 停顿的 release+DWARF 定点状态采集。
- 如果证据指向 futex/IPI，最小计数器或状态 dump；不能用持续高频日志扰动时序。

## 10.4 当前运行路径

```text
user store -> LoongArch StorePageFault/PME
 -> kernel trap page-fault handler
 -> wateros_mm::handle_cow_fault(aspace, VA)
 -> LA with_user_aspace_mut_and_page_flush(lock)
 -> handle_cow_fault_no_flush
    -> COW: mutate/copy PTE, true
    -> already W+D+U leaf: concurrent loser, true
    -> otherwise false
 -> true: local Page flush + request_tlb_shootdown
 -> retry user instruction
```

## 10.5 数据结构与状态机

| 状态 | PTE/TLB | 事件 | 下一状态 |
|---|---|---|---|
| Shared COW | PTE COW, TLB D=0 | CPU A write | A 持锁解析 COW |
| Resolved | PTE W+D, peers may retain D=0 TLB | CPU B 已取 PME 后持锁 | 新分支 returns handled |
| Retried | local/peer invalidated | B 重执行 store | 正常写入 |
| Genuine fault | unmapped/RO/non-user/non-leaf | write | false→SIGSEGV |

## 10.6 API、ABI、协议和格式

- 无公共 Rust API 变化；新增 PTE 私有 flag queries。
- Linux signal frame ABI 由 `cc62fc75` 及 padding `2cc13e52` 处理。
- LA token 编码/ASID ABI 未变。
- 评测输出协议不变；禁止脚本正文日志不影响结果 markers。

## 10.7 期望行为与当前行为对照 [REQUIRED]

| 场景 | 期望 | 当前证据 | 状态 |
|---|---|---|---|
| 并发 COW loser | flush/retry，不 kill | 代码符合，probe支持 | PARTIAL runtime |
| 真只读写 fault | SIGSEGV | 条件严格保留 false | UNVERIFIED regression |
| 完整 LA buildstorm | status/run OK | 旧修复一次成功；当前修复后停顿 | PARTIAL |
| RV buildstorm | 稳定完成 | 旧对话 3/3，无本交接原始日志 | STALE/PARTIAL |

# 11. 决策记录

## 11.1 已采用决策 [REQUIRED]

### DEC-001：把真实页表状态作为并发 COW loser 的判据

- 背景：PME 后等锁期间另一个 CPU 可完成同一 COW。
- 选择：只有 level-0 leaf 且 W+D+U 时，把 fault 视为 handled 并触发 flush/retry。
- 理由：与捕获现场完全一致，且不放宽到真 RO/unmapped。
- 影响：仅 LoongArch MM；RISC-V 不编译此文件。
- 回滚条件：出现可重复的真正 protection fault 被吞，或该分支导致 livelock。

### DEC-002：ASID 首次 CPU 运行做保守 full local flush

- 理由：LA PGDL+ASID switch 不隐式清旧 translation；ASID teardown 未必在未来 CPU 本地执行。
- 代价：每个 aspace/CPU 首次调度一次 full flush。

### DEC-003：调试证据与评测脚本合规分离

- 内核可保留必要 fault 数据到独立诊断构建，但 main 不打印脚本正文。
- 测试脚本只按原始恢复文件替换，不修改判定逻辑。

## 11.2 用户明确作出的决策

- 有效的 ASID 修复先进入 main，再继续看 UEFI/hashbrown。
- 正确性优先于性能；最终每架构三轮。
- 评测脚本正文打印必须删除并进入 main。

## 11.3 被否决或失败的方案 [REQUIRED]

### REJ-001：用修改测试脚本绕过/提前结束

原本脚本线上不可修改；任何 `cargo; tee`、省略嵌套 QEMU 或伪造 output 都不能解决内核。

### REJ-002：仅凭编译日志卡点归因具体 crate

Cargo 行显示“下一个开始的 crate”，不等于该 crate 独占耗时。hashbrown/UEFI 卡点在不同
运行出现，表明是时序/系统状态而非稳定单 crate 编译 bug。

### REJ-003：仅用高侵入 debug 运行判断竞态

GDB/调试功能会改变时序；一次 hashbrown 卡点曾因 debug 时序越过。应先普通 full 稳定
复现，再低侵入冻结/读取。

### REJ-004：把内存不足当已证实根因

用户监控全程低于3G guest-related usage；冻结时所有 vCPU idle、host available 足够。
磁盘 95% 是环境风险，但不能解释 guest 无 runnable task。

### REJ-005：修改 LoongArch clone 参数顺序

已核对 glibc LA clone 汇编：`a3=child_tid`, `a4=tls` 与当前 special case 匹配；不要重做。

# 12. 调查结论、事实与假设

## 12.1 已验证事实 [REQUIRED]

- [VERIFIED] 失败样本发生 `StorePageFault`, `ecode=4`, VA `0x70040b40`，随后
  `SIGSEGV signal not delivered`。证据：fault-probe log lines 271–274。
- [VERIFIED] 冻结页表 walk 的 leaf 为 V/P/W/D/PLV3，说明真实 PTE 已允许该 store。
  证据：`notes/pte-walk-analysis.md`。
- [VERIFIED] 旧 `handle_cow_fault_no_flush` 只调用 `handle_cow_page`；COW bit 被其他
  CPU 清掉后会 false。证据：`snapshots/relevant-commits.patch`。
- [VERIFIED] main 包含 `705950b4` 的 W+D+U race retry 和 `9bc17023` ASID flush。
- [VERIFIED] main 不包含 `SCRIPT_BODY_FLAT` 代码路径；旧日志正文已从 handoff 副本删除。
- [VERIFIED] 当前无相关 QEMU/GDB、loop 或测试镜像 mount。

## 12.2 直接观察

- [OBSERVED] ASID fix 后 full-1 在 535.81s 完成、`status=OK run=OK`。
- [OBSERVED] 相同代码的 full-2 在 build 后期出现 SIGSEGV 并未结束。
- [OBSERVED] COW fix 后 verify1 没出现 SIGSEGV，但停在 `Compiling hashbrown v0.17.1`；
  冻结检查时 12 个 vCPU 都在 idle（旧对话现场，未保留 GDB transcript）。
- [OBSERVED] 早先 hashbrown freeze 中 timer interrupt 仍增长，用户 CPU 使用率个位数。

## 12.3 推断

- [INFERRED, high confidence] 捕获的 SIGSEGV 是并发 COW loser 对陈旧 D=0 translation
  产生 PME，等锁后看到已 resolved PTE，被旧实现误判为真 fault。
- [INFERRED] hashbrown/UEFI 随机停顿至少可能包含另一个竞态；COW fix 不能据此判定失败，
  也不能判定全问题完成。
- [INFERRED] LA 特有程度更高，优先检查 LA IPI/TLB/task wake 路径而非 RV 公共逻辑。

## 12.4 待验证假设 [REQUIRED]

### HYP-001：missed futex wake/等待生命周期导致无 runnable task

- 支持：all vCPU idle；早期状态有 futex wait queues；卡点变化。
- 反证：没有最新 hang 的 waiter/owner 对应表。
- 最小实验：冻结后 dump scheduler active tasks、futex queues、wait attempts/returns/wakes，
  对比最后 uaddr/scope 和 task state；不先改代码。

### HYP-002：LoongArch software IPI/TLB shootdown race

- 支持：LA 特有、并发地址空间/TLB 高压；ASID/COW 修复都与 shootdown 相邻。
- 反证：timer/IPI 基础中断工作；尚无 CPU 卡在 shootdown wait 的栈。
- 最小实验：在 hang 时读 IPI pending/ack counters、shootdown queue 和 per-CPU state。

### HYP-003：主机磁盘/内存压力只是放大时序

- 支持：root FS 95%、swap used。
- 反证：guest vCPU 全 idle而非 host QEMU 忙；available memory 尚可。
- 最小实验：记录每次 host `vmstat/iostat/df` 与 guest state，不先改内核。

## 12.5 未知信息

- 最新 verify1 hang 的完整 task/futex/IPI dump 未保存。
- 当前 `c413c4b9` 重建产物的三轮 LA 结果未知。
- 线上决赛镜像与 pub+恢复脚本除脚本外的精确差异未知。

# 13. 问题复现与调试状态

## 13.1 最小复现 [REQUIRED for bug/debug tasks]

### 前置条件

- current main；从 `os/` 重建 `kernel-la`。
- QEMU 9.2.1 local build。
- 从压缩 pub 镜像产生新的可写副本并写入恢复脚本；不要 snapshot，不复用污染副本。

### 精确命令

以下命令重建测试输入；先确认目标文件不是唯一原镜像：

```bash
cd /home/zhitian/project/WaterOS_refactor/os
make kernel-la
gzip -dc /home/zhitian/Downloads/sdcard-la-pub.img.gz > sdcard-la-race-test.img
debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' sdcard-la-race-test.img
debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' sdcard-la-race-test.img
debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' sdcard-la-race-test.img
/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64 \
  -kernel kernel-la -m 36G -nographic -smp 12 \
  -drive file=sdcard-la-race-test.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 -no-reboot \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc \
  2>&1 | tee /tmp/wateros-la-main-full-N.log
```

`debugfs set_inode_field` 的 mode 语法若本机版本拒绝，先 `stat`；恢复脚本本身为 0755，
可使用经过验证的 `debugfs` 写入流程。不要修改脚本正文。

### 输入/测试数据

- `sdcard-la-pub.img.gz`: SHA-256 `2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2`。
- `buildstorm_testcode.recovered.sh`: 7,549 bytes, mode 0755, SHA-256
  `84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`。

### 期望结果

`TOOLCHAIN_RESULT status=OK`, `MINIBUILD_RESULT status=OK`, final
`BUILDSTORM_RESULT mode=multi status=OK ... run=OK`，busybox bringup all commands finished。

### 实际结果

- 一次旧修复样本完整成功（535.81s）。
- 一次出现 `SIGSEGV signal not delivered`。
- fault-probe 复现相同 SIGSEGV 并抓到 W+D+U leaf。
- COW fix 样本停在 hashbrown，没有最终 marker。

### 稳定性

竞态；曾在 hashbrown、UEFI 等不同进度行停顿。不能用单轮成功作为最终稳定结论。

## 13.2 错误签名

```text
[trap][probe] fault cause=Exception(StorePageFault) raw=0x40000 ecode=0x4
sepc=0x101d2afc stval=0x70040b40 ... satp=0x700096b2b2000
[trap] SIGSEGV signal not delivered — killing user task
```

停顿签名：最后一行常为 `Compiling hashbrown v0.17.1` 或 UEFI 相关编译；host QEMU
CPU 很低；冻结时所有 12 vCPU 处于内核 idle，guest 不继续输出。

## 13.3 最后已知正常与首次已知异常

- 最后完整正常：`9bc17023` 状态下 full-1，2026-08-13 00:16，535.81s。
- 同状态异常：full-2，2026-08-13 00:39，SIGSEGV。
- 当前 COW fix 首样本：2026-08-13 01:02，被 timeout/清理终止，hashbrown stall。

## 13.4 调试器状态

- 当前无 GDB/QMP session、端口或 stopped VM。
- 旧 probe 使用 QEMU GDB/QMP 冻结并手工 walk 页表；原 transcript 未保存，结构化结果
  在 `notes/pte-walk-analysis.md`。
- 重建时可给 QEMU 加 `-S -gdb tcp::1234`，但这会改变时序；优先普通复现后动态 attach
  或 QMP stop。

## 13.5 插桩与临时调试改动

- fault probe 内核：`/tmp/kernel-la-fault-probe-1f56057a`, SHA-256
  `0a299fb65afd761fc5c9be74956df2cc21e44d26571980877c4516bd505f1298`。
- probe 代码不在 main；main 当前没有 `[trap][probe]` 临时改动。
- 脚本正文打印也已从 main 移除。

## 13.6 性能分析状态

本任务未运行 perf/QEMU plugin。此前系统调用和热点分析属于性能任务。full-1 的 535.81s
只用于正确性/粗略回归，不与旧 800s 不同镜像样本直接比较。

## 13.7 内核 / QEMU / 裸机专项快照 [CONDITIONAL]

### 架构与构建

| 项 | 值 |
|---|---|
| target | LoongArch64 unknown-none final kernel；guest glibc toolchain builds LA musl app |
| profile | kernel final/release；buildstorm app release optimized |
| feature selection | `kernel-la-final` / Makefile final config |
| linker/target spec | repository current LA target; exact rustflags from Make output需重建记录 |
| SMP | local/online LA 12 vCPU；RV 8 vCPU |

### 启动链

Host x86_64 → QEMU 9.2.1 `qemu-system-loongarch64` → WaterOS `kernel-la` →
virtio-blk-pci rootfs → busybox bringup → recovered buildstorm → cargo builds
ArceOS helloworld → guest launches image-bundled LA QEMU via edk2 pflash+ESP → `Hello, world!`。

### QEMU 精确状态

线上用户提供：

```bash
qemu-system-loongarch64 -kernel kernel-la -m 36G -nographic -smp 12 \
  -drive file=sdcard-la.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 -no-reboot \
  -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc
```

本地用相同参数和绝对 QEMU 9.2.1 路径；当前无 PID。退出：正常测试结束或对已确认 PID
发送 SIGTERM；不得 `pkill qemu`。

### 设备树与设备

- QEMU machine 默认为 LA `virt`；内核从 boot DTB 读取完整 memory range（`2a7d2712`）。
- root block: `virtio-blk-pci`; network: `virtio-net-pci`; RTC UTC。
- 线上 QEMU 不显式 `-machine`/`-cpu`; 不自行加 QEMU 11 专属参数。

### 磁盘镜像与文件系统

- raw ext filesystem image，uncompressed 15,032,385,536 bytes。
- 当前 `sdcard-la-tlb-test.img` 的脚本 inode size 7,549，mtime 00:54:47；它是测试副本，
  不是干净源。`sdcard-la-pub.img` 当前脚本 size 7,590，说明也不是恢复脚本原样。
- 新轮次必须从 gzip 解压新目标；镜像很大，不做整镜像备份。

### Trap / 异常现场

| CSR/字段 | 值 |
|---|---|
| exception | StorePageFault / PME, ecode 4, raw 0x40000 |
| ERA/sepc | `0x101d2afc` |
| BADV/stval | `0x70040b40` |
| SP/TP | `0x313d6630` / `0x313d7620` |
| token | `0x700096b2b2000`, ASID 7, PGDL `0x96b2b2000` |
| task | pid156/tid1214/task1410/Member, parent task308, Running User |
| PTE | leaf `0x40000009621ea19f`, decoded V/P/W/D/user, NX |

### 调度、内存与中断状态

- 早期 hang：12 idle tasks running，无 non-idle Ready/Running；约46 active TCBs。
- futex snapshot（旧对话，无文件）：14 queues/14 waiters，robust26，wait attempts5194,
  returns5033, wake calls18467, woken2028；最后 wait timed out，最后 wake woke0。
- timer interrupt 在 hang 中继续增长，约100Hz；不支持“timer完全死掉”。
- guest/host 内存没有接近 OOM；root filesystem 95% 满是噪声风险。

### GDB 连接

当前 `N/A`。未来仅在普通 hang 复现后 attach；记录完整 QEMU `-gdb`/port、ELF hash、
所有 CPU backtrace、scheduler/futex/IPI globals 到新 log，避免只留对话叙述。

### 串口、trace 与性能产物

串口 logs 在本 handoff `logs/`；无 perf/trace/plugin 数据。ANSI 色码保留；旧脚本正文已删。

# 14. 构建、测试与验证矩阵

## 14.1 验证状态说明 [REQUIRED]

`PASSED` 只用于保存有完成 marker 的运行；`PARTIAL` 表示代码/build 通过但 runtime 未完成；
被 timeout/SIGTERM 终止的测试不算通过。

## 14.2 构建矩阵 [REQUIRED]

| ID | 命令/CWD | 时间 | HEAD | Exit | 结果/日志 |
|---|---|---|---|---:|---|
| BUILD-001 | `make kernel-la`, `os/` | 2026-08-13 00:53左右 | `705950b4` | 0 | 对话执行记录；当前无独立 build log |
| BUILD-002 | `make kernel-la` + `make kernel-rv` | 2026-08-13 00:37前 | `1f56057a` | 0 | 对话执行记录；验证移除正文打印 |
| BUILD-003 | current HEAD rebuild | 未运行 | `c413c4b9` | N/A | 必须在 NEXT-001 前执行 |

## 14.3 自动测试矩阵 [REQUIRED]

| ID | 状态 | Commit | 关键结果 | 日志 |
|---|---|---|---|---|
| TEST-LA-001 | PASSED | `9bc17023` descendant/pre-COW | `status=OK elapsed=535.81 run=OK` | `logs/wateros-la-tlb-firstuse-full-1.log` |
| TEST-LA-002 | FAILED | 同上 | SIGSEGV; no final result | `logs/wateros-la-tlb-firstuse-full-2.log` |
| TEST-LA-003 | FAILED/reproducer | `1f56057a` + probe | PME + W/D/U PTE + SIGSEGV | `logs/wateros-la-fault-probe-full-3.log` |
| TEST-LA-004 | INCOMPLETE | `705950b4` | no SIGSEGV before hashbrown stall; timeout SIGTERM | `logs/wateros-la-cow-race-fix-verify1.log` |
| TEST-RV-3X | STALE/PASSED in conversation | signal/memory fixes | user/agent observed 3 successes | raw logs not in handoff |

## 14.4 手工验证

- GDB/QMP PTE walk：完成，见 notes；建立 COW race 机制。
- `rg SCRIPT_BODY_FLAT os/src/user_bringup_busybox.rs`：exit 1/无输出，符合预期。
- `git diff --check`：exit 0（仅 tracked，handoff 尚未 tracked）。

## 14.5 失败和已知红灯

### FAIL-001：COW loser 被当作 SIGSEGV

- 由任务引入：no；修复目标。
- 摘要：PME 时真实 leaf 已 W+D+U，旧 COW handler false。
- 原因：已高置信确定并修复。
- 是否阻塞完成：该具体机制不再阻塞代码，但需 runtime 验证。

### FAIL-002：hashbrown/UEFI 随机停顿

- 由任务引入：unknown；ASID/COW 修复前后均见相似停顿。
- 摘要：CPU 利用率低、串口不再前进、all vCPU idle。
- 原因：未确定；HYP-001/002。
- 阻塞：阻塞 LoongArch 3/3 和任务 DONE。

## 14.6 未运行的测试 [REQUIRED]

| 测试/命令 | 原因 | 风险 | 何时必须运行 |
|---|---|---|---|
| current main `make kernel-la` | handoff 导出不扩大运行 | 旧 kernel-la 不可用于结论 | 下一步立即 |
| current main LA full 3x | 每轮约9分钟且仍可能 hang | 无稳定性结论 | 修复验收前 |
| current main RV full 3x | 本轮改动 LA-only，但最终用户要求两架构 | 公共后续改动可能回归 | 若动公共代码或最终验收 |
| genuine RO fault regression | 无独立测试 | 可能误吞 SIGSEGV | COW fix review/验收前 |

## 14.7 覆盖矩阵

| Requirement | 验证 | 状态 |
|---|---|---|
| REQ-F-001 | TEST-LA-001..004 | PARTIAL |
| REQ-F-002 | TEST-LA-003 + code review | PARTIAL |
| REQ-COMP-001 | TEST-LA-001 | PARTIAL/current stale |
| REQ-COMP-002 | TEST-RV-3X | PARTIAL/stale |
| REQ-SAFE-001 | commit+rg | VERIFIED |
| REQ-BUILD-001 | Makefile code check | VERIFIED code / runtime unrun |

## 14.8 回归检查

- 受影响：LA MM page fault/TLB，busybox bringup logging。
- 潜在回归：真 protection fault 被重试；过度 TLB flush；shootdown storm；RV 公共 signal ABI。
- 已执行：LA build；一次旧 full success；fault probe。
- 未覆盖：current full 3x、真 RO store、ASID wrap/reuse stress。

## 14.9 性能与资源对比 [CONDITIONAL]

| 指标 | 基线 | 当前 | 样本 | 结论 |
|---|---:|---:|---:|---|
| LA elapsed | 旧 main约800s（用户叙述，不同环境） | 535.81s at TEST-LA-001 | 1 | 不可作严格 A/B；只说明没有明显灾难回归 |
| host QEMU CPU at hang | N/A | 个位数/idle | 多次观察 | 支持 guest wait/runnable 问题，不证明根因 |

# 15. 产物、日志与数据清单

## 15.1 交接目录结构

```text
loongarch-race-fix/
├── HANDOFF.md
├── logs/        # 四份筛除脚本正文的串口日志
├── snapshots/   # Git/进程/mount/patch/hash
├── artifacts/   # 大产物不复制的策略说明
└── notes/       # PTE walk 结构化分析
```

## 15.2 产物清单 [REQUIRED]

| ID | 路径 | 类型/用途 | 大小 | SHA-256 | tracked | 可重建 |
|---|---|---|---:|---|---|---|
| ART-001 | `logs/wateros-la-tlb-firstuse-full-1.log` | 成功 serial | 33,279 | `675bbbe6…` | no | yes |
| ART-002 | `logs/wateros-la-tlb-firstuse-full-2.log` | SIGSEGV serial | 17,566 | `0724a837…` | no | race-dependent |
| ART-003 | `logs/wateros-la-fault-probe-full-3.log` | fault probe | 16,408 | `4d880a88…` | no | race-dependent |
| ART-004 | `logs/wateros-la-cow-race-fix-verify1.log` | post-fix stall | 12,228 | `5968bc35…` | no | race-dependent |
| ART-005 | `snapshots/relevant-commits.patch` | 3 commits patch | 6,686 | `c852eaf0…` | no | yes |
| ART-006 | `notes/pte-walk-analysis.md` | page-table evidence | 1,376 | `4e3fc9fd…` | no | only from notes |

所有 log hash 也在 `snapshots/logs-sha256.txt`。

## 15.3 输入数据与测试夹具

| 数据 | 路径/来源 | hash | 限制 | 获取/生成 |
|---|---|---|---|---|
| LA pub gzip | `~/Downloads/sdcard-la-pub.img.gz` | `2c411447…` | 2.16GB，不复制 | 用户提供 |
| recovered script | `~/Downloads/buildstorm_testcode.recovered.sh` | `84d63101…` | 禁止正文泄露/逆向 | 用户提供，原样写镜像 |
| test image | `os/sdcard-la-tlb-test.img` | 未计算（15GB成本） | ignored/可污染 | gzip副本+脚本替换 |

## 15.4 外部链接或资源

| 资源 | 用途 | 版本 | 联网 | 本地替代 |
|---|---|---|---|---|
| Git remotes | 历史/协作 | 当前 remote refs | 非本任务必须 | 本地 89 commits ahead |
| QEMU source build | 线上等价 emulator | 9.2.1 | no | `~/qemu_9_2_1/.../build/` |

# 16. 临时运行状态与不可由 Git 保存的状态

## 16.1 正在运行的进程

`N/A` — 导出核验未发现相关 QEMU、GDB 或 QMP。完整系统 process snapshot 已保存；
其中包含采集命令自身，不应误判为活动测试。

## 16.2 端口、socket 与会话

`N/A` — 无已知 GDB/QMP/tmux 会话需保留。接管时仍需重新 `ss`/`ps` 核验。

## 16.3 Mount、loop、容器和虚拟机

- 测试相关 mount/loop/QEMU：none；`snapshots/losetup.txt` 为空。
- 常规 host mounts 在 `snapshots/findmnt.txt`；不要卸载未知 mount。
- 容器：本任务未使用/未创建。

## 16.4 临时目录、缓存与锁

- `/tmp/wateros-la-*.log` 原始日志仍存在；handoff 已复制筛选版。
- `/tmp/kernel-la-fault-probe-1f56057a` 仍存在。
- 两个 `/tmp/wateros-*` Git worktree 仍受保护；不要因空间不足直接删除。
- 无已知 test lock/PID file。只有确认没有进程、mount、唯一证据依赖后才清理。

## 16.5 不能迁移的现场

原 stopped VM/GDB 内存已消失；仅页表 walk 记录可迁移。要获得新现场必须重新复现，
冻结 VM 后立即保存 GDB/QMP transcript，而不是只在聊天中报告。

# 17. 外部依赖、权限与秘密边界

## 17.1 外部服务

本任务运行验证不依赖网络服务；Cargo 在 guest offline。Git remotes 仅未来同步使用。

## 17.2 凭据要求

- 本地构建/测试：不需要凭据。
- push GitLab/GitHub：可能需要 SSH key/token，由用户安全配置；本交接不含值。
- 绝不写入：token、私钥、Cookie、评测机凭据、完整受限脚本正文。

## 17.3 网络与代理

运行可离线；未记录代理；不得为本任务擅自联网更新工具链或依赖。

## 17.4 权限与不可逆操作

| 操作 | 批准 | 风险 | 替代 |
|---|---|---|---|
| 写新测试镜像副本 | 已在任务范围 | 磁盘空间 | 从 gzip 明确目标名 |
| 覆盖 pub/决赛镜像 | 需要明确批准 | 丢失干净基准 | 创建新副本 |
| commit handoff/修复 | 用户要求进入 main 时可做；先审查 | 混入既有 untracked | 精确 path commit |
| push/删除 worktree/stash | 需要明确批准 | 外部/数据损失 | 保留并报告 |

# 18. 工作日志与尝试历史

| 时间 | ID | 操作 | 结果 | 证据/影响 |
|---|---|---|---|---|
| 2026-08-12 20:50 | WORK-001 | Linux signal frame + DTB memory | commits进入main | `cc62fc75`,`2a7d2712` |
| 2026-08-12 22:30 | WORK-002 | 初始化 trap/signal padding | 修复未初始化 ABI padding | `2cc13e52` |
| 2026-08-13 00:12 | WORK-003 | LA ASID first-use flush | 一轮成功、一轮仍SIGSEGV | `9bc17023`, full-1/2 |
| 2026-08-13 00:37 | WORK-004 | 删除脚本正文打印 | main 合规 | `1f56057a` |
| 2026-08-13 00:41–00:53 | WORK-005 | fault probe+PTE walk | 定位并发 COW loser | fault-probe log/note |
| 2026-08-13 00:53 | WORK-006 | COW race retry | build passed、进入main | `705950b4` |
| 2026-08-13 01:02 | WORK-007 | post-fix full verify1 | 无SIGSEGV但hashbrown stall | verify1 log；被停止 |
| 2026-08-13 01:09 | WORK-008 | 清理运行现场并导出 handoff | 无QEMU/mount；证据已打包 | 本目录 |

## 18.1 最后一个完成的动作 [REQUIRED]

- 动作：按 export prompt 采集 Git/运行/日志/输入 hash，筛除旧脚本正文，写 handoff。
- 结果：证据包完整；handoff 状态 PARTIAL，因为 LA 3/3 未完成。
- HEAD/dirty：`main@c413c4b9`；导出前 13 untracked，导出后再加本目录。
- 下一动作未执行原因：导出模式要求暂停扩大实现；全量测试成本高且用户当前只要求 handoff。

## 18.2 被中断的动作

- `wateros-la-cow-race-fix-verify1`：停在 hashbrown 后由 timeout/SIGTERM 终止。
- 无半写源码；可能留下测试镜像和 `/tmp` log，均已明确列出。

# 19. 风险、阻塞项与技术债

## 19.1 风险矩阵 [REQUIRED]

| ID | 风险 | 概率 | 影响 | 证据 | 缓解/触发 | Owner |
|---|---|---:|---:|---|---|---|
| RISK-001 | 独立 hashbrown/UEFI race 未修 | H | H | post-fix stall | 3轮+冻结现场 | 接管者 |
| RISK-002 | COW W+D+U 判定隐藏真 fault | L/M | H | 新分支无专项test | 真RO regression | 接管者 |
| RISK-003 | host root 95% 导致噪声/失败 | M | M | df snapshot | 预留空间，不删受保护数据 | 用户/接管者 |
| RISK-004 | 使用过期 kernel-la 得假结论 | H | H | mtime 8/11 | 每轮先hash/重建 | 接管者 |
| RISK-005 | 误提交既有 untracked/受限日志 | M | H | 13 files+旧script markers | 精确 path add；正文已筛 | 接管者 |

## 19.2 当前阻塞项

### BLOCK-001：LoongArch buildstorm 随机无进展

- 阻塞：LA 3/3、线上稳定出分、恢复性能优化。
- 原因：不确定；已修 COW SIGSEGV 不足以解释 all-idle hang。
- 证据：verify1 hashbrown stall；此前 UEFI/hashbrown 多位置复现。
- 解除：当前 main 连续3次完整 status/run OK，或新的 hang 有明确根因并修复后3/3。
- 可并行：只读审查 LA futex/wait/IPI/TLB call path；不要同时跑抢CPU QEMU。
- 是否需用户：当前不需要。

## 19.3 已知技术债

| ID | 技术债 | 本次处理 | 原因/风险 | 后续入口 |
|---|---|---|---|---|
| DEBT-001 | LA COW race 无 focused unit/stress test | no | kernel并发环境难host复现 | `pagetable.rs` test/support |
| DEBT-002 | hang 诊断状态主要在旧对话 | partial | transcript未保存 | NEXT-002保存结构化dump |
| DEBT-003 | TLB shootdown full flush较保守 | no | 正确性优先 | 稳定后再性能化 |

# 20. 后续工作队列与决策树

## 20.1 优先级队列 [REQUIRED]

### NEXT-001 — P0 — 重建当前 main 并做 LoongArch 第 1 轮干净 full

- 前置：只读差异核验通过；无其他 QEMU；至少约16GB可用存储用于新 raw image。
- 命令：第 13.1 节命令，日志 `/tmp/wateros-la-main-full-1.log`。
- 预期：`BUILDSTORM_RESULT ... status=OK ... run=OK`。
- 证据：保存内核 SHA、镜像来源 hash、完整命令、exit、结果行和 log hash。
- 完成判定：第1轮完整结束；成功后同一代码/每轮干净镜像继续轮2/3。
- 失败分支：无输出且 host CPU低超过合理窗口，不盲等；进入 NEXT-002。
- 禁止：复用旧 `kernel-la`、修改恢复脚本、并行跑 RV/其他 QEMU。

### NEXT-002 — P0 — 对下一次 all-idle hang 保存可复核现场

- 目标：区分 missed futex wake、丢 runnable task、IPI/shootdown wait。
- 精确动作：确认 PID；只对该 QEMU attach/stop；保存 all-CPU backtrace、scheduler task
  states、futex queues/counters、IPI/shootdown counters、timer counters、host `ps/vmstat/iostat`。
- 相关 symbol：scheduler ready queue；futex wait/wake registry；LA IPI/shootdown；
  `with_user_aspace_mut_and_page_flush`。
- 完成：至少能回答“谁应唤醒哪个 task/CPU，状态在哪一步丢失”。
- 失败：debug attach 使问题消失，则降低插桩，仅加原子 ring/counters 后普通复现。
- 禁止：先大改 scheduler/IRQ 架构；不要无 PID `pkill`。

### NEXT-003 — P1 — 回归与最终验收

- LA 修复后3/3；若只改 LA cfg-specific code，至少一次 RV smoke 并核对既有3/3；若改公共
  scheduler/futex/signal/MM API，则 RV 也重新3/3。
- 最终 `make all` 并确认 repo root/os 交付物只能是 `kernel-la`,`kernel-rv`。
- 更新本 handoff 的测试矩阵、hash、HEAD 和状态；只有全部完成才改 DONE/READY。

## 20.2 决策树 [REQUIRED]

```text
核验 main/HEAD/进程 -> 重建 kernel-la -> 干净镜像 full
├─ status=OK + run=OK
│  ├─ 继续直到 LA 3/3
│  └─ 按改动范围做 RV 回归与 make all 交付物检查
├─ 再次 SIGSEGV，且 PME/PTE 与旧签名相同
│  ├─ 保存当前 PTE/TLB/shootdown 现场
│  └─ 审查 705950b4 的 outer flush 是否实际执行
├─ hashbrown/UEFI 无进展且 all CPU idle
│  ├─ 执行 NEXT-002，不先猜 crate
│  └─ futex证据 -> wait/wake；IPI证据 -> LA shootdown
└─ host OOM/IO error/空间不足
   ├─ 标记环境失败，不算 kernel regression
   └─ 只在明确目标/可重建条件下释放空间
```

## 20.3 完成路径

```text
NEXT-001 -> (failure: NEXT-002 -> minimal fix -> NEXT-001)
         -> LA 3/3 -> scoped RV regression -> make all audit
         -> update handoff -> final review
```

## 20.4 可并行项

| 任务 | 可并行 | 冲突资源 | 合并 |
|---|---|---|---|
| 静态阅读 futex/IPI | 镜像解压 | repo源码/磁盘IO | 先不编辑 |
| RV full | 不建议与 LA full 并行 | CPU/IO，会污染时间/时序 | 串行执行 |

# 21. 不要重复、不要破坏与受保护状态

## 21.1 不要重复的失败尝试 [REQUIRED]

- REJ-001：不要通过修改脚本或 cat output 来“修”线上问题。
- REJ-002：不要把最后一条 Cargo `Compiling ...` 当成唯一耗时 crate 根因。
- REJ-003：不要一开始就启用高侵入 debug；先普通复现，再冻结读取。
- REJ-004：不要再次修改 LA clone a3/a4 映射；已核对正确。
- REJ-005：不要仅凭单轮成功宣称竞态解决；用户要求3轮。

## 21.2 不要覆盖的修改 [REQUIRED]

- `snapshots/preexisting-untracked-files.txt` 中全部13项：用户/其他任务现场。
- `/tmp/wateros-elf-cache-128m` 的 dirty report、`/tmp/wateros-perf-tools/final_test_case`。
- 其他硬件 port worktree 和5个 stash。
- `~/Downloads` 原始 gzip/恢复脚本；只读作为输入。

## 21.3 禁止执行的操作

未经用户明确授权，不 reset/clean/强制 checkout/rebase/stash/删除分支或 worktree，
不覆盖镜像，不 kill 未确认 PID，不卸载未知 mount，不 push，不泄露脚本正文，不把未跑
测试写成通过。

## 21.4 必须保持的不变量

- main `make all` 的最佳 final 产物只为 `kernel-rv`、`kernel-la`。
- 原始恢复脚本判定语义不变；nested QEMU 必须实际 boot hello world。
- LA 修复尽量架构隔离；公共路径修改后 RV 必须重验。
- 真 RO/unmapped fault 仍产生正确 SIGSEGV。

# 22. 开放问题

## 22.1 必须由用户决定

`None`。只有需要删除受保护大文件释放空间、推送远端或扩大到公共调度器重构时再请求。

## 22.2 可由接管者通过调查解决

| ID | 问题 | 最小调查 | 影响 |
|---|---|---|---|
| Q-TECH-001 | post-fix hang 是否仍复现 | current main clean full 1–3 | 决定是否继续代码修复 |
| Q-TECH-002 | all-idle 的 waiter/waker 在哪丢失 | NEXT-002 dump | 决定 futex vs IPI 路径 |
| Q-TECH-003 | COW fix 是否吞真RO fault | focused protection test | 安全回归 |

## 22.3 已回答但容易被误解的问题

| ID | 结论 | 来源 | 常见误解 |
|---|---|---|---|
| Q-CLOSED-001 | `make all` 代码已限制只保留两个 final 内核 | `os/Makefile:517-541` | 当前旧 kernel 文件等于当前HEAD产物（错误） |
| Q-CLOSED-002 | DTB memory end 修复已在 main | `2a7d2712` | 36G guest 必然host OOM（未证实） |
| Q-CLOSED-003 | 捕获 SIGSEGV 的 PTE 已 W+D+U | PTE walk | 这是普通 unmapped/RO fault（错误） |
| Q-CLOSED-004 | COW SIGSEGV 修复不等于 hashbrown hang 修复 | TEST-LA-004 | 无 SIGSEGV即全部通过（错误） |

# 23. 接管启动协议

## 23.1 接管者只读检查清单 [REQUIRED]

- [ ] 读取所有适用 `AGENTS.md` 和本 HANDOFF 全文。
- [ ] 确认 CWD、repo root、main、完整 HEAD、base/upstream。
- [ ] 检查 staged/unstaged/untracked/required ignored 和其他 worktree。
- [ ] 核对三个关键 commits 仍为 HEAD 祖先、关键 symbol 未变化。
- [ ] 核对 `kernel-la` 是否为当前 HEAD；默认视为不是并重建。
- [ ] 检查 QEMU/GDB、端口、mount、loop、磁盘空间。
- [ ] 验证 gzip/恢复脚本 hash，不显示脚本正文。
- [ ] 写 Import discrepancy report；无 blocking 差异才执行 NEXT-001。
- [ ] 不 reset、clean、checkout、stash、commit 或覆盖既有修改。

## 23.2 接管差异报告格式

```markdown
## Import discrepancy report
| ID | Handoff says | Actual state | Severity | Impact | Action |
|---|---|---|---|---|---|
| DIFF-001 | ... | ... | info/warn/blocking | ... | ... |
```

## 23.3 新对话最小启动消息

```text
接管任务 `loongarch-race-fix`。

交接文件：docs/agent/handoffs/loongarch-race-fix/HANDOFF.md

先按“接管启动协议”完成只读核验并报告差异。不要 reset、clean、checkout、
revert、stash、commit 或覆盖已有修改。核验后直接从 NEXT-001 继续；若实际状态
使 NEXT-001 不成立，先更新差异报告和交接文件。
```

# 24. 交接完整性审计

## 24.1 必填项检查 [REQUIRED]

- [x] 用户目标、要求、纠正、非目标和禁止项已记录。
- [x] 每项核心要求有 ID、状态、实现和证据/缺口。
- [x] branch、HEAD、base、dirty、staged/unstaged/untracked/ignored 已记录。
- [x] 用户/其他任务文件与本 handoff 已区分。
- [x] 修改文件、symbol、前后行为、决策和失败方案已记录。
- [x] 事实/观察/推断/假设、完成/部分/未开始已分开。
- [x] 复现、错误签名、QEMU/镜像/trap/页表现场已记录。
- [x] 构建/测试均有对应状态；缺失 exit/log 已明确，不伪造。
- [x] 非 Git 进程、mount、loop、worktree、临时数据已记录。
- [x] 产物含路径、大小、hash或无法计算理由。
- [x] 下一步有命令、成功/失败分支、停止条件和禁止副作用。
- [x] 无凭据；受限脚本正文已从交接日志副本删除。
- [x] 写后重新读取并与当前状态比较（见最终审计命令）。

最终状态保存在 `snapshots/final-git-status-porcelain-v2.txt`；交接目录所有文件的
SHA-256 清单保存在 `snapshots/file-manifest-sha256.txt`（该 manifest 不自哈希）。

## 24.2 缺失信息报告 [REQUIRED]

| 缺失 | 原因 | 影响 | 补全 |
|---|---|---|---|
| old RV 3/3 raw logs | 未纳入当前 `/tmp` 证据集 | RV结论标STale | 重新跑或找原结果文件 |
| latest hang GDB transcript | 当时只在会话报告 | 根因未闭环 | NEXT-002 保存 |
| current HEAD build/full results | 导出未扩大测试 | handoff只能PARTIAL | NEXT-001 |
| exact source chat ID | 运行时未提供 | 无技术影响 | 产品元数据获取 |

## 24.3 新鲜度与可信度

- Handoff：`PARTIAL`；整体可信度：`high`（Git/代码/日志），运行完成度不足。
- 最先过期：进程、HEAD、dirty、磁盘空间、镜像内容。
- 立即重验：第 1.4 和 23.1；current kernel hash/build。
- 未解决内部矛盾：无。单轮成功与后续失败被明确视为竞态样本，不互相覆盖。

## 24.4 最终自检结论 [REQUIRED]

```text
HANDOFF_READY=yes
REASON=竞态证据、已合入修复、未解决停顿、受保护现场和精确下一步均可独立恢复；任务本身仍为PARTIAL
FIRST_NEXT_ACTION=核验 main/HEAD/进程后重建当前 kernel-la，并用新解压镜像运行第1轮原始 full
CURRENT_BRANCH=main
CURRENT_HEAD=c413c4b97e36f91b1b4ba960209d00509f45ebd5
WORKING_TREE=dirty
BLOCKING_USER_DECISION=none
```

# 附录 A：命令与原始输出索引

| ID | 命令 | 输出 | Exit | 时间/备注 |
|---|---|---|---:|---|
| CMD-001 | `git status --porcelain=v2 --branch` | `snapshots/git-status-porcelain-v2.txt` | 0 | 2026-08-13 01:08 |
| CMD-002 | `git worktree list --porcelain` | `snapshots/git-worktrees.txt` | 0 | 同上 |
| CMD-003 | `git show ... 9bc17023 705950b4 1f56057a` | `snapshots/relevant-commits.patch` | 0 | 同上 |
| CMD-004 | `ps -eo ...` | `snapshots/processes.txt` | 0 | 无相关VM；含采集进程自身 |
| CMD-005 | `findmnt`; `losetup -a` | snapshots | 0 | 无相关mount/loop |
| CMD-006 | `sha256sum` inputs/logs | snapshots hash files | 0 | 脚本正文未读取到本文 |
| CMD-007 | CodeGraph explore COW/ASID call path | 对话输出；symbol见6.4 | 0 | current source |

# 附录 B：关键对话证据

| ID | 来源摘要 | 影响 |
|---|---|---|
| CHAT-001 | 用户：“修复和验收计划”后要求 implement | 授权内核修复和验证 |
| CHAT-002 | 用户纠正“改脚本只能定位，线上脚本不能改” | 禁止 workaround |
| CHAT-003 | 用户要求从 Downloads 干净解压并替换 recovered script | 固定测试输入流程 |
| CHAT-004 | 用户观察 LA hashbrown/UEFI 随机通过/停住 | 竞态而非单crate根因 |
| CHAT-005 | 用户要求每架构总共3轮 | 最终 DoD |
| CHAT-006 | 用户要求删除脚本打印并提交 main | 合规 commit `1f56057a` |

# 附录 C：术语与缩写

| 术语 | 含义 |
|---|---|
| PME | LoongArch Page Modification Exception；本现场 ecode 4 |
| COW | copy-on-write |
| PGDL | LoongArch 低地址用户页表基址 |
| ASID | address-space identifier |
| shootdown | 请求其他 CPU 无效化指定地址空间的 TLB translation |
| W/D/U | writable / dirty / user PTE 属性 |

# 附录 D：机器可读摘要

```yaml
summary:
  goal: "stabilize LoongArch buildstorm on QEMU 9.2.1 without script workarounds"
  status: "PARTIAL"
  branch: "main"
  head: "c413c4b97e36f91b1b4ba960209d00509f45ebd5"
  dirty: true
  first_next_action: "rebuild kernel-la and run clean full test 1/3"
requirements:
  done: [REQ-SAFE-001, REQ-BUILD-001, REQ-DOC-001]
  partial: [REQ-F-001, REQ-F-002, REQ-COMP-001, REQ-COMP-002]
  todo: ["LoongArch 3/3", "post-fix hang root cause if reproduced"]
tests:
  passed: [TEST-LA-001]
  failed: [TEST-LA-002, TEST-LA-003]
  incomplete: [TEST-LA-004]
  not_run: ["current-main-build", "current-main-LA-3x", "current-main-RV-regression"]
blockers: [BLOCK-001]
protected_paths:
  - "snapshots/preexisting-untracked-files.txt entries"
  - "/tmp/wateros-elf-cache-128m"
  - "/tmp/wateros-perf-tools"
```
