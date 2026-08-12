<!--
Codex Task Handoff Template v1

填写规则：
1. 所有标记为 [REQUIRED] 的字段必须填写。
2. 不适用时写 `N/A — 原因`，不知道时写 `UNKNOWN — 缺失原因 — 获取方法`，不要直接删除字段。
3. 关键结论必须带状态标签与证据。不要把推测写成事实。
4. 长日志、完整补丁、二进制产物放到本文件同级目录中；HANDOFF.md 只保留摘要、路径、生成命令和校验值。
5. 严禁写入密码、token、私钥、Cookie、完整授权头或其他秘密。
6. 时间统一使用 ISO 8601 并注明时区，例如 `2026-08-12T13:20:00+09:00`。
-->

---
handoff_schema: "codex-task-handoff/v1"
handoff_id: "<HOF-YYYYMMDD-task-slug>"
task_id: "<stable-task-id>"
task_title: "<任务名称>"
task_status: "<NOT_STARTED | IN_PROGRESS | BLOCKED | READY_FOR_REVIEW | DONE>"
handoff_status: "<READY | PARTIAL | BLOCKED>"
handoff_reason: "<为何需要换对话/主机/工作区>"
created_at: "<ISO-8601>"
updated_at: "<ISO-8601>"
freshness_deadline: "<何时以后必须重新验证，或 N/A>"
created_by: "<agent/model/chat>"
source_chat_id: "<session/thread id or UNKNOWN>"
source_chat_title: "<title or UNKNOWN>"
target_chat_id: "<UNKNOWN until imported>"
repository_name: "<repo name>"
repository_root: "<absolute path>"
workspace_path: "<absolute path>"
workspace_kind: "<local | worktree | remote | cloud | container>"
git_branch: "<branch | detached>"
git_head: "<full commit hash>"
git_base_branch: "<base branch | UNKNOWN>"
git_base_commit: "<merge-base/full hash | UNKNOWN>"
working_tree_state: "<clean | dirty | UNKNOWN>"
primary_platform: "<host/target architecture>"
confidentiality: "<public | internal | sensitive>"
---

# 0. 交接契约与阅读规则

## 0.1 接管者必须遵守的事实优先级 [REQUIRED]

出现冲突时，按下列顺序处理：

1. 用户在接管后的最新明确指令。
2. 当前仓库、当前文件内容、当前 Git 状态和重新执行命令得到的结果。
3. 当前作用域内的 `AGENTS.md`、`AGENTS.override.md`、项目文档和规范。
4. 本交接中带有 `[VERIFIED]` 且仍未过期的结论。
5. 本交接中的 `[OBSERVED]`、`[INFERRED]`、`[HYPOTHESIS]`。
6. 旧对话中的未验证叙述。

若本交接与实际仓库不一致，不得静默选择其一；必须在“接管差异报告”中记录差异及影响。

## 0.2 状态标签 [REQUIRED]

- `[USER-REQ]`：用户明确提出的要求。
- `[USER-CORRECTION]`：用户对先前理解或方案作出的纠正。
- `[VERIFIED]`：在当前或明确记录的仓库状态上通过命令、代码检查或测试确认。
- `[OBSERVED]`：直接看到的现象或输出，但尚未证明原因。
- `[INFERRED]`：基于多个已知事实做出的推断。
- `[DECISION]`：已经采用并应继续遵循的设计或实施决定。
- `[HYPOTHESIS]`：尚待实验验证的解释。
- `[TODO]`：尚未完成的工作。
- `[BLOCKED]`：受外部条件或未解决问题阻塞。
- `[STALE]`：曾经成立，但当前状态可能已变化，必须重新验证。
- `[N/A]`：该字段不适用于本任务。
- `[UNKNOWN]`：当前无法确定；必须同时写明缺失原因和获取方法。

## 0.3 证据书写格式 [REQUIRED]

关键结论建议使用：

```text
[VERIFIED] <结论>
Evidence:
- Command/File/Source: <命令、文件、symbol、用户消息或日志路径>
- State: branch=<...>, HEAD=<...>, dirty=<yes/no>
- Time: <ISO-8601>
- Result: <exit code/关键输出/行号/测试名>
```

## 0.4 本交接的边界

- 本文件是否包含所有旧对话信息：<YES | NO，说明缺失范围>
- 本文件是否根据当前仓库重新核验：<YES | PARTIAL | NO>
- 未能读取的来源：<列表或 None>
- 未能运行的命令：<列表或 None>
- 因安全、权限、时间或成本而省略的检查：<列表或 None>

# 1. 执行摘要

## 1.1 一句话状态 [REQUIRED]

<用 1–3 句话说明已经做到哪里、当前卡在哪里、接下来第一步是什么。>

## 1.2 任务目标 [REQUIRED]

<最终要实现、修复、调查或交付的结果。避免仅写模块名。>

## 1.3 当前完成度 [REQUIRED]

- 总体估计：<0–100%，并说明估计依据>
- 已完成：
  - <...>
- 部分完成：
  - <...>
- 未开始：
  - <...>
- 当前阻塞：
  - <...>

## 1.4 接管后第一项操作 [REQUIRED]

```bash
<第一条应执行的精确命令，或第一处应只读检查的文件/symbol>
```

目的：<该操作要确认什么>

预期分支：

- 若结果 A：<下一步>
- 若结果 B：<下一步>
- 若出现其他结果：<停止条件与记录方式>

## 1.5 最大风险

<当前最可能导致返工、数据丢失、回归或错误结论的风险。>

## 1.6 当前是否需要用户决定

- <None，或列出必须由用户决定的问题>
- 在得到决定前可安全继续的工作：<...>

# 2. 任务来源与对话上下文

## 2.1 来源对话

| 字段 | 内容 |
|---|---|
| 对话/线程 ID | <...> |
| 对话标题 | <...> |
| 起始时间 | <...> |
| 最后更新时间 | <...> |
| 与其他对话的关系 | <原始 / fork / resume / 手工转交 / UNKNOWN> |
| 旧对话是否仍可访问 | <YES/NO/UNKNOWN> |

## 2.2 用户意图摘要 [REQUIRED]

<从完整对话中提炼用户真正希望达成的结果，包括为什么要做、优先级和最终使用场景。>

## 2.3 用户要求与纠正时间线 [REQUIRED]

不要只保留最终一句要求；应保留会影响实施的原始要求、后续修改、否决和优先级变化。

| ID | 时间/回合 | 类型 | 规范化后的要求 | 原始措辞摘要或来源指针 | 当前状态 |
|---|---|---|---|---|---|
| UREQ-001 | <...> | USER-REQ | <...> | <...> | accepted/changed/done/open |
| UCOR-001 | <...> | USER-CORRECTION | <...> | <...> | applied/open |
| UDEC-001 | <...> | USER-DECISION | <...> | <...> | binding/open |

## 2.4 用户偏好与协作方式

仅记录会影响工作质量或沟通方式的偏好。

- 输出形式：<完整代码 / patch / 命令 / 解释深度 / 文档格式>
- 修改策略：<最小改动 / 可重构 / 保持兼容 / 先分析后实现>
- 验证偏好：<必须运行哪些测试或展示哪些证据>
- 沟通约束：<不要重复询问、不要省略完整命令等>
- 明确反感或禁止的行为：<...>

## 2.5 仍存在的歧义

| ID | 歧义 | 已有证据 | 当前采用的临时解释 | 错误解释的影响 | 解决方式 |
|---|---|---|---|---|---|
| AMB-001 | <...> | <...> | <...> | <...> | <...> |

# 3. 目标、范围与完成定义

## 3.1 最终目标 [REQUIRED]

<从用户视角描述最终结果。>

## 3.2 本次工作范围 [REQUIRED]

### In scope

- <...>

### Out of scope

- <...>

### Deferred

- <推迟到后续任务的内容及原因>

## 3.3 非目标与禁止性要求 [REQUIRED]

- 不需要实现：<...>
- 不允许改变：<...>
- 不允许引入：<...>
- 必须保持兼容：<...>
- 用户明确否决：<...>

## 3.4 交付物清单 [REQUIRED]

| ID | 交付物 | 路径/位置 | 格式 | 当前状态 | 验收方式 |
|---|---|---|---|---|---|
| DEL-001 | <...> | <...> | <...> | TODO/PARTIAL/DONE | <...> |

## 3.5 完成标准 / Definition of Done [REQUIRED]

只有全部强制项满足时，任务才可标记为 DONE。

- [ ] 功能行为：<...>
- [ ] 兼容性：<...>
- [ ] 构建：<完整命令与预期>
- [ ] 自动测试：<完整命令与预期>
- [ ] 手工验证：<步骤与预期>
- [ ] 性能/资源：<阈值或“无回归”的比较方式>
- [ ] 文档：<...>
- [ ] 清理：<无临时调试代码/无无关改动/格式化>
- [ ] Git 状态：<允许 dirty 或必须 clean>
- [ ] 用户要求：<所有 UREQ/UCOR 已追踪>
- [ ] 已知限制已明确记录。

## 3.6 停止条件

遇到以下情况应停止扩大修改并报告，而不是继续猜测：

- <可能破坏用户数据或工作区>
- <需要用户作出架构选择>
- <验证环境与交接严重不一致>
- <需要秘密、生产权限或不可逆操作>
- <其他>

# 4. 需求追踪矩阵

每条要求使用稳定 ID；不得仅在自然语言段落中隐含存在。

| ID | 类型 | 要求 | 来源 | 优先级 | 状态 | 实现位置 | 验证证据 | 缺口/备注 |
|---|---|---|---|---|---|---|---|---|
| REQ-F-001 | functional | <...> | UREQ-001 | MUST | TODO | <file::symbol> | <test/log> | <...> |
| REQ-NF-001 | non-functional | <...> | <...> | SHOULD | PARTIAL | <...> | <...> | <...> |
| REQ-COMP-001 | compatibility | <...> | <...> | MUST | DONE | <...> | <...> | <...> |
| REQ-PERF-001 | performance | <...> | <...> | SHOULD | TODO | <...> | <...> | <...> |
| REQ-SAFE-001 | safety/security | <...> | <...> | MUST | DONE | <...> | <...> | <...> |
| REQ-DOC-001 | documentation | <...> | <...> | SHOULD | TODO | <...> | <...> | <...> |
| REQ-NOT-001 | prohibition | <...> | <...> | MUST NOT | ACTIVE | <...> | <...> | <...> |

# 5. 已加载的持久指令与规范

## 5.1 指令文件清单 [REQUIRED]

| 加载顺序 | 路径 | 类型 | 适用范围 | 关键规则摘要 | 文件状态/哈希 |
|---|---|---|---|---|---|
| 1 | `<~/.codex/AGENTS.md>` | global | all | <...> | <mtime/hash or UNKNOWN> |
| 2 | `<repo/AGENTS.md>` | repo | repo | <...> | <...> |
| 3 | `<subdir/AGENTS.override.md>` | local override | subdir | <...> | <...> |

## 5.2 其他规范与文档

| 路径/来源 | 作用 | 与当前任务相关的内容 | 是否已核验 |
|---|---|---|---|
| `<README/PLANS/design docs>` | <...> | <...> | YES/NO |

## 5.3 指令冲突及处理

<列出不同层级规则的冲突、最终采用哪条以及原因。若无，写 None。>

# 6. 项目与仓库地图

## 6.1 仓库集合 [REQUIRED]

适用于单仓库、多仓库、子模块或相邻用户态仓库。

| Repo ID | 名称 | 根目录 | 当前分支 | HEAD | 作用 | 是否有修改 |
|---|---|---|---|---|---|---|
| REP-01 | <...> | <...> | <...> | <...> | <...> | yes/no |

## 6.2 目录与模块职责

```text
<仅列出与任务有关的目录树；不要粘贴整个大型仓库>
```

| 路径 | 模块职责 | 与任务的关系 | 主要入口/symbol |
|---|---|---|---|
| `<path>` | <...> | <...> | `<symbol>` |

## 6.3 架构概览

- 系统边界：<...>
- 主要组件：<...>
- 数据流：<...>
- 控制流：<...>
- 并发模型：<...>
- 错误传播：<...>
- 持久化/存储模型：<...>
- 兼容层/API/ABI：<...>

## 6.4 当前任务涉及的关键 symbol [REQUIRED]

| Symbol | 路径 | 类型 | 当前作用 | 调用者 | 被调用项 | 相关不变量 |
|---|---|---|---|---|---|---|
| `<Type::method>` | `<file>` | fn/type/module | <...> | <...> | <...> | <...> |

## 6.5 重要不变量

- INV-001：<任何修改都必须保持的性质>
- INV-002：<...>

# 7. 环境快照

## 7.1 主机与工作区 [REQUIRED]

| 项目 | 值 |
|---|---|
| 当前时间与时区 | <...> |
| Hostname | <...> |
| OS / release | <...> |
| Host kernel | <...> |
| Host architecture | <...> |
| Shell | <...> |
| CWD | <...> |
| Codex 运行位置 | local/worktree/remote/cloud/container |
| 文件系统/挂载特殊情况 | <...> |
| 网络可用性 | <...> |
| 权限/approval/sandbox 限制 | <...> |

## 7.2 工具链与版本 [REQUIRED]

| 工具 | 完整版本 | 获取命令 | 是否任务关键 |
|---|---|---|---|
| Git | <...> | `git --version` | yes |
| Rust/Cargo | <...> | `rustc -Vv`; `cargo -V` | <...> |
| QEMU | <...> | `<qemu> --version` | <...> |
| GDB/LLDB | <...> | `<gdb> --version` | <...> |
| Make/CMake/Ninja | <...> | <...> | <...> |
| Python/Node/Java | <...> | <...> | <...> |
| 交叉工具链 | <...> | <...> | <...> |

## 7.3 依赖与锁定状态

- 包管理器：<...>
- lockfile：<path + hash>
- vendored dependencies：<...>
- submodules：<...>
- system packages：<仅列任务关键项>
- 已知依赖漂移：<...>

## 7.4 相关环境变量

只写变量名和脱敏后的值；秘密写 `<REDACTED: reason>`。

```text
RUSTUP_TOOLCHAIN=<...>
CARGO_TARGET_DIR=<...>
HTTP_PROXY=<redacted host/port if needed>
HTTPS_PROXY=<...>
ALL_PROXY=<...>
PATH=<仅保留任务关键段>
<OTHER>=<...>
```

## 7.5 环境建立与恢复命令

```bash
<从干净环境恢复到可构建/可复现状态的命令>
```

## 7.6 已知环境差异

- 与 CI 的差异：<...>
- 与用户本地环境的差异：<...>
- 与旧对话运行环境的差异：<...>
- 会影响结果的差异：<...>

# 8. Git 与工作区精确快照

## 8.1 仓库身份 [REQUIRED]

```text
repo_root:
workspace_path:
worktree_kind:
branch:
HEAD:
upstream:
base_branch:
merge_base:
detached_HEAD:
```

## 8.2 `git status` [REQUIRED]

Command:

```bash
git status --porcelain=v2 --branch
```

Output:

```text
<完整输出，或引用 logs/git-status.txt>
```

## 8.3 最近提交

```text
<git log --oneline --decorate --graph -n N 的输出或日志路径>
```

## 8.4 Staged 修改 [REQUIRED]

| 路径 | 状态 | 说明 |
|---|---|---|
| <...> | A/M/D/R | <...> |

摘要：

```text
<git diff --cached --stat>
```

## 8.5 Unstaged 修改 [REQUIRED]

| 路径 | 状态 | 说明 |
|---|---|---|
| <...> | M/D | <...> |

摘要：

```text
<git diff --stat>
```

## 8.6 Untracked 文件 [REQUIRED]

| 路径 | 大小 | SHA-256 | 是否任务必需 | 是否含敏感信息 | 如何迁移/重建 |
|---|---:|---|---|---|---|
| <...> | <...> | <...> | yes/no | yes/no/unknown | <...> |

## 8.7 Required ignored 文件 [REQUIRED]

`.gitignore` 中但任务运行依赖的文件不会天然由 Git 保证。

| 路径/模式 | 作用 | 是否可再生成 | 生成命令 | 是否包含秘密 | 迁移要求 |
|---|---|---|---|---|---|
| <...> | <...> | yes/no | <...> | yes/no | <...> |

## 8.8 Worktree、stash、submodule、LFS

```text
git worktree list --porcelain:
<...>

git stash list:
<...>

git submodule status --recursive:
<...>

git lfs status:
<... or N/A>
```

## 8.9 Remote 信息（脱敏）

```text
<remote name + sanitized URL；移除 URL 中的用户名、token 和密码>
```

## 8.10 补丁与快照

| 文件 | 覆盖内容 | 生成命令 | SHA-256 | 备注 |
|---|---|---|---|---|
| `snapshots/working-tree.patch` | tracked staged+unstaged | `git diff --binary HEAD -- .` | <...> | <...> |
| `snapshots/staged.patch` | staged only | `git diff --cached --binary` | <...> | <...> |
| `snapshots/untracked-manifest.tsv` | untracked metadata | <...> | <...> | 不含文件内容 |

## 8.11 用户已有修改与所有权边界 [REQUIRED]

明确区分哪些修改由本任务产生，哪些是用户或其他 agent 已有，避免接管者覆盖。

| 路径/范围 | 所有者 | 是否允许修改 | 识别依据 |
|---|---|---|---|
| <...> | user/previous-agent/current-task/unknown | yes/no/ask | <...> |

# 9. 文件与代码修改清单

## 9.1 已修改文件 [REQUIRED]

| 路径 | Git 状态 | 相关 symbol/范围 | 修改目的 | 修改前行为 | 修改后行为 | 完成度 | 验证 | 风险 |
|---|---|---|---|---|---|---|---|---|
| `<file>` | M/A/D/R | `<symbol>` | <...> | <...> | <...> | DONE/PARTIAL | <test/log> | <...> |

## 9.2 新增文件

| 路径 | 作用 | 是否应提交 | 生成/维护方式 | 依赖者 |
|---|---|---|---|---|
| <...> | <...> | yes/no | <...> | <...> |

## 9.3 删除或重命名

| 原路径 | 新路径 | 原因 | 兼容性影响 | 是否已更新全部引用 |
|---|---|---|---|---|
| <...> | <...> | <...> | <...> | yes/no |

## 9.4 未修改但必须先阅读的文件

| 路径 | 原因 | 关键位置/symbol |
|---|---|---|
| <...> | <...> | <...> |

## 9.5 生成文件和构建产物

| 路径 | 来源命令 | 是否可删除 | 是否需迁移 | 当前有效性 |
|---|---|---|---|---|
| <...> | <...> | yes/no | yes/no | valid/stale |

## 9.6 代码审查关注点

- <容易漏看的边界条件>
- <unsafe/并发/生命周期/错误处理>
- <API/ABI 兼容>
- <临时调试代码>
- <潜在无关改动>

# 10. 当前实现状态与行为模型

## 10.1 已完成实现 [REQUIRED]

### IMPL-001：<名称>

- 状态：`[VERIFIED]` / `[PARTIAL]`
- 文件与 symbol：<...>
- 做了什么：<...>
- 为什么这样做：<...>
- 关键不变量：<...>
- 已覆盖场景：<...>
- 未覆盖场景：<...>
- 验证证据：<...>

## 10.2 部分实现

### IMPL-P-001：<名称>

- 已完成：<...>
- 缺失：<...>
- 当前代码是否可编译：<...>
- 临时状态/占位符：<...>
- 继续入口：<file::symbol>
- 完成该项所需步骤：<...>

## 10.3 尚未开始

- TODO-001：<...>
- 前置条件：<...>
- 预计影响范围：<...>

## 10.4 当前运行路径

```text
<入口>
  -> <模块/函数>
  -> <关键分支>
  -> <状态变化>
  -> <输出/错误>
```

## 10.5 数据结构与状态机

- 关键类型：<...>
- 字段语义：<...>
- 生命周期/所有权：<...>
- 状态转换：<...>
- 并发同步：<...>
- 锁顺序：<...>
- 失败回滚：<...>

## 10.6 API、ABI、协议和格式

| 接口/格式 | 当前约定 | 兼容要求 | 已知偏差 |
|---|---|---|---|
| <...> | <...> | <...> | <...> |

## 10.7 期望行为与当前行为对照 [REQUIRED]

| 场景 | 输入/前置状态 | 期望 | 当前实际 | 差异状态 | 证据 |
|---|---|---|---|---|---|
| <...> | <...> | <...> | <...> | fixed/open/unknown | <...> |

# 11. 决策记录

## 11.1 已采用决策 [REQUIRED]

### DEC-001：<决策标题>

- 状态：`[DECISION] accepted`
- 日期：<...>
- 决策者：<user/agent/team>
- 背景：<为什么需要决策>
- 候选方案：
  1. <方案 A>
  2. <方案 B>
- 最终选择：<...>
- 理由：<技术、兼容、成本、用户要求>
- 代价与副作用：<...>
- 影响的文件/API：<...>
- 验证方式：<...>
- 何时允许重新考虑：<触发条件>
- 来源：<用户消息/代码/文档>

## 11.2 用户明确作出的决策

| ID | 决策 | 约束范围 | 是否可由接管者更改 |
|---|---|---|---|
| UDEC-001 | <...> | <...> | no/ask |

## 11.3 被否决或失败的方案 [REQUIRED]

### REJ-001：<方案>

- 尝试内容：<...>
- 使用的代码/命令：<...>
- 结果：<...>
- 否决原因：<...>
- 证据：<log/test>
- 是否完全无效：<yes/no>
- 重新考虑所需的新证据：<...>
- 不应重复的部分：<...>

# 12. 调查结论、事实与假设

## 12.1 已验证事实 [REQUIRED]

- `[VERIFIED] FACT-001`：<结论>
  - Evidence：<...>
  - Valid at：branch=<...>, HEAD=<...>, time=<...>

## 12.2 直接观察

- `[OBSERVED] OBS-001`：<现象>
  - Reproduction：<...>
  - Frequency：<always/intermittent/N of M>
  - Log：<...>

## 12.3 推断

- `[INFERRED] INF-001`：<推断>
  - Supporting facts：<FACT/OBS IDs>
  - Alternative explanations：<...>
  - Confidence：<low/medium/high>

## 12.4 待验证假设 [REQUIRED]

### HYP-001：<假设>

- 假设：<...>
- 支持证据：<...>
- 反对证据：<...>
- 最小验证实验：<命令/代码改动>
- 预期结果：
  - 若成立：<...>
  - 若不成立：<...>
- 实验副作用：<...>
- 状态：TODO/RUNNING/REJECTED/CONFIRMED

## 12.5 未知信息

| ID | 未知项 | 为什么重要 | 获取方法 | 阻塞程度 |
|---|---|---|---|---|
| UNK-001 | <...> | <...> | <...> | none/partial/full |

# 13. 问题复现与调试状态

## 13.1 最小复现 [REQUIRED for bug/debug tasks]

### 前置条件

- <...>

### 精确命令

```bash
<完整、可直接复制的命令；包含必要的 cwd、参数和环境变量>
```

### 输入/测试数据

- 路径：<...>
- SHA-256：<...>
- 生成方法：<...>

### 期望结果

```text
<...>
```

### 实际结果

```text
<...>
```

### 稳定性

- 复现次数：<N/M>
- 首次/最近复现时间：<...>
- 是否受随机种子、并发、机器负载影响：<...>

## 13.2 错误签名

- 错误类型：<panic/segfault/assertion/test failure/hang/wrong output>
- 首个异常点：<...>
- 最后一个正常点：<...>
- 关键日志：<...>
- Backtrace：<path/excerpt>
- Core dump：<path/hash/N/A>
- Exit code/signal：<...>

## 13.3 最后已知正常与首次已知异常

| 状态 | Commit/版本 | 命令 | 结果 | 证据 |
|---|---|---|---|---|
| Last known good | <...> | <...> | <...> | <...> |
| First known bad | <...> | <...> | <...> | <...> |

## 13.4 调试器状态

- 调试器与版本：<...>
- 被调试二进制：<path + build-id/hash>
- 符号文件：<...>
- 启动/连接命令：<...>
- 远程端口：<...>
- 断点/观察点：<...>
- 当前停点：<file:line / symbol / address>
- 线程/进程/hart：<...>
- Backtrace：<...>
- 关键寄存器/变量：<...>
- 调试脚本：<...>
- 该状态是否可迁移：<yes/no；如何重建>

## 13.5 插桩与临时调试改动

| 路径/symbol | 插桩内容 | 是否仍存在 | 移除条件 |
|---|---|---|---|
| <...> | <...> | yes/no | <...> |

## 13.6 性能分析状态

- 目标指标：<latency/throughput/CPU/memory/hotspot>
- 基线命令：<...>
- 当前命令：<...>
- 采样方法：<perf/ftrace/QEMU plugin/custom counters>
- 数据路径：<...>
- 当前热点：<...>
- 置信限制：<虚拟化、采样频率、符号缺失等>

## 13.7 内核 / QEMU / 裸机专项快照 [CONDITIONAL]

### 架构与构建

| 项目 | 值 |
|---|---|
| Host arch | <...> |
| Target arch | <riscv64/loongarch64/...> |
| Rust target / target spec | <...> |
| Toolchain | <nightly/date/components> |
| Build profile/features | <...> |
| Linker script | <...> |
| Kernel ELF | <path/hash/build-id> |
| Raw kernel image | <path/hash> |
| User-space/test image | <path/hash> |

### 启动链

```text
Firmware/BIOS -> Bootloader/OpenSBI -> Kernel entry -> early init -> ...
```

- Firmware 路径与版本：<...>
- Kernel entry symbol/address：<...>
- 内存布局：<...>
- 页表模式：<...>
- 启动 hart/CPU 数：<...>

### QEMU 精确状态

```bash
<完整 qemu-system-* 命令，不省略参数>
```

| 项目 | 值 |
|---|---|
| QEMU version | <...> |
| machine | <...> |
| cpu | <...> |
| memory | <...> |
| smp | <...> |
| firmware/bios | <...> |
| kernel | <...> |
| initrd | <...> |
| block devices | <...> |
| virtio/mmio/pci devices | <...> |
| serial/monitor | <...> |
| gdb port / `-S` | <...> |
| trace/plugin options | <...> |
| random/rtc/icount | <...> |

### 设备树与设备

- DTB 来源：<QEMU generated / file>
- DTB 路径/hash：<...>
- 关键节点：<virtio, interrupt controller, memory, chosen>
- 解析后的关键地址/IRQ：<...>
- 实际设备与驱动绑定：<...>

### 磁盘镜像与文件系统

| 项目 | 值 |
|---|---|
| image path/hash | <...> |
| generation command | <...> |
| partition/filesystem | <...> |
| mount/loop device | <...> |
| journal/features | <...> |
| test files | <...> |
| clean rebuild command | <...> |

### Trap / 异常现场

```text
architecture:
hart/cpu:
task/pid:
privilege mode:
cause/scause:
epc/sepc/era:
tval/stval/badv:
status/sstatus/crmd:
satp/pgdl/pgdh:
sp:
ra:
syscall number/args:
faulting instruction:
page-table translation:
last serial logs:
```

- 现场是否仍可重建：<...>
- 最小触发路径：<...>
- 相关 symbol：<...>

### 调度、内存与中断状态

- 当前任务/进程：<...>
- 调度器状态：<...>
- 锁与中断使能状态：<...>
- frame allocator/heap 状态：<...>
- 页表映射：<...>
- timer/外部中断：<...>
- 当前 syscall：<...>
- 潜在竞态：<...>

### GDB 连接

```bash
<启动 QEMU 命令>
<启动 GDB 命令>
<target remote ...>
<必要的 set architecture / symbol-file / add-symbol-file>
```

### 串口、trace 与性能产物

| 产物 | 路径 | 生成命令 | 关键结论 | SHA-256 |
|---|---|---|---|---|
| serial log | <...> | <...> | <...> | <...> |
| QEMU trace | <...> | <...> | <...> | <...> |
| profile data | <...> | <...> | <...> | <...> |
| disassembly | <...> | <...> | <...> | <...> |

# 14. 构建、测试与验证矩阵

## 14.1 验证状态说明 [REQUIRED]

- 验证所对应的 branch/HEAD：<...>
- 验证时工作区是否 dirty：<...>
- dirty 时具体包含哪些文件：<...>
- 测试结果是否仍适用于当前 HEAD：<yes/no/partial>
- 最后验证时间：<...>

## 14.2 构建矩阵 [REQUIRED]

| ID | 命令 | CWD | 环境/target | 时间 | Exit | 结果 | 日志 | 对应 HEAD |
|---|---|---|---|---|---:|---|---|---|
| BUILD-001 | `<...>` | <...> | <...> | <...> | 0 | PASS | <...> | <...> |

## 14.3 自动测试矩阵 [REQUIRED]

| ID | 命令/测试名 | 覆盖要求 | 时间 | Exit | Pass/Fail/Skip | 关键输出 | 日志 | 对应 HEAD |
|---|---|---|---|---:|---|---|---|---|
| TEST-001 | `<...>` | REQ-F-001 | <...> | <...> | <...> | <...> | <...> | <...> |

## 14.4 手工验证

| ID | 步骤 | 预期 | 实际 | 证据 | 结果 |
|---|---|---|---|---|---|
| MAN-001 | <...> | <...> | <...> | <screenshot/log> | PASS/FAIL |

## 14.5 失败和已知红灯

### FAIL-001：<测试/命令>

- 命令：<...>
- 是否由本任务引入：<yes/no/unknown>
- 错误摘要：<...>
- 完整日志：<...>
- 初步原因：<...>
- 是否阻塞完成：<...>
- 下一验证步骤：<...>

## 14.6 未运行的测试 [REQUIRED]

| 测试/命令 | 未运行原因 | 风险 | 何时必须运行 |
|---|---|---|---|
| <...> | 权限/成本/环境/未知命令 | <...> | <...> |

禁止把“未运行”写成“预计会通过”。

## 14.7 覆盖矩阵

| Requirement ID | 验证方法 | 当前证据 | 状态 |
|---|---|---|---|
| REQ-F-001 | TEST-001 | <...> | VERIFIED/PARTIAL/UNVERIFIED |

## 14.8 回归检查

- 受影响模块：<...>
- 可能的回归面：<...>
- 已执行回归测试：<...>
- 未覆盖回归：<...>

## 14.9 性能与资源对比 [CONDITIONAL]

| 指标 | 基线 | 当前 | 差异 | 测量方法 | 样本数 | 结论 |
|---|---:|---:|---:|---|---:|---|
| <...> | <...> | <...> | <...> | <...> | <...> | <...> |

# 15. 产物、日志与数据清单

## 15.1 交接目录结构

```text
<task-id>/
├── HANDOFF.md
├── logs/
├── snapshots/
├── artifacts/
└── notes/
```

## 15.2 产物清单 [REQUIRED]

| ID | 路径 | 类型 | 用途 | 生成命令/来源 | 大小 | SHA-256 | tracked/ignored | 是否可重建 |
|---|---|---|---|---|---:|---|---|---|
| ART-001 | <...> | log/patch/bin/image/data | <...> | <...> | <...> | <...> | <...> | yes/no |

## 15.3 输入数据与测试夹具

| 数据 | 路径/来源 | 版本/hash | 许可/隐私限制 | 获取/生成方式 |
|---|---|---|---|---|
| <...> | <...> | <...> | <...> | <...> |

## 15.4 外部链接或资源

不要仅写链接；应同时记录其用途、版本、访问要求和本地替代。

| 资源 | 用途 | 版本/日期 | 是否必须联网 | 本地缓存/替代 |
|---|---|---|---|---|
| <...> | <...> | <...> | yes/no | <...> |

# 16. 临时运行状态与不可由 Git 保存的状态

## 16.1 正在运行的进程

| PID | 命令 | CWD | 作用 | 是否必须保留 | 重建命令 | 安全停止方法 |
|---:|---|---|---|---|---|---|
| <...> | <...> | <...> | <...> | yes/no | <...> | <...> |

## 16.2 端口、socket 与会话

| 类型 | 标识 | 占用者 | 用途 | 重建方式 |
|---|---|---|---|---|
| TCP/Unix/tmux/screen | <...> | <...> | <...> | <...> |

## 16.3 Mount、loop、容器和虚拟机

| 类型 | ID/设备 | 映射/挂载点 | 作用 | 状态 | 重建与清理命令 |
|---|---|---|---|---|---|
| loop | `/dev/loopX` | <...> | <...> | active | <...> |
| mount | <...> | <...> | <...> | <...> | <...> |
| container | <id/name> | <volumes> | <...> | <...> | <...> |
| QEMU/VM | <pid> | <...> | <...> | <...> | <...> |

## 16.4 临时目录、缓存与锁

- 临时目录：<...>
- 编译缓存：<...>
- 锁文件：<...>
- PID 文件：<...>
- 会阻止下一次运行的残留：<...>
- 可安全删除的条件：<...>

## 16.5 不能迁移的现场

<列出调试器停点、内存中的数据、终端状态等，并给出重建步骤。>

# 17. 外部依赖、权限与秘密边界

## 17.1 外部服务

| 服务 | 用途 | 环境 | 当前状态 | 所需权限 | 无服务时的替代 |
|---|---|---|---|---|---|
| <...> | <...> | dev/test/prod | <...> | <...> | <...> |

## 17.2 凭据要求

只记录“需要什么凭据”和“从哪里安全取得”，不记录值。

- 需要的凭据类型：<...>
- 安全获取位置：<...>
- 当前环境是否已配置：<yes/no/unknown>
- 最小权限：<...>
- 绝不应写入交接的秘密：<...>

## 17.3 网络与代理

- 网络访问要求：<...>
- 允许/禁止域：<...>
- 代理设置：<脱敏>
- 离线可执行部分：<...>
- 已知网络故障：<...>

## 17.4 权限与不可逆操作

| 操作 | 是否需要批准 | 风险 | 替代方案 |
|---|---|---|---|
| sudo/写系统目录/推送/部署/删除数据 | yes/no | <...> | <...> |

# 18. 工作日志与尝试历史

按时间顺序记录“做了什么—为什么—结果—下一步”，避免新对话重复探索。

| 时间 | Work ID | 操作/修改 | 目的 | 结果 | 证据 | 对后续的影响 |
|---|---|---|---|---|---|---|
| <...> | WORK-001 | <...> | <...> | success/fail/partial | <...> | <...> |

## 18.1 最后一个完成的动作 [REQUIRED]

- 动作：<...>
- 结果：<...>
- 发生时的 HEAD/dirty 状态：<...>
- 下一动作为什么尚未执行：<...>

## 18.2 被中断的动作

- 命令/编辑：<...>
- 中断位置：<...>
- 是否可能留下半成品：<...>
- 检查/恢复方法：<...>

# 19. 风险、阻塞项与技术债

## 19.1 风险矩阵 [REQUIRED]

| ID | 风险 | 概率 | 影响 | 证据 | 缓解措施 | 触发信号 | Owner |
|---|---|---|---|---|---|---|---|
| RISK-001 | <...> | L/M/H | L/M/H | <...> | <...> | <...> | <...> |

## 19.2 当前阻塞项

### BLOCK-001：<标题>

- 阻塞内容：<...>
- 阻塞的任务/要求：<...>
- 原因是否确定：<yes/no>
- 已有证据：<...>
- 解除条件：<...>
- 可并行进行的工作：<...>
- 是否需要用户/外部人员：<...>

## 19.3 已知技术债

| ID | 技术债 | 本次是否处理 | 延后原因 | 风险 | 后续入口 |
|---|---|---|---|---|---|
| DEBT-001 | <...> | yes/no | <...> | <...> | <...> |

# 20. 后续工作队列与决策树

## 20.1 优先级队列 [REQUIRED]

### NEXT-001 — P0 — <标题>

- 目标：<...>
- 前置条件：<...>
- 精确命令/文件：
  ```bash
  <...>
  ```
- 相关 symbol：<...>
- 预期结果：<...>
- 产出证据：<test/log/diff>
- 完成判定：<...>
- 失败分支：<...>
- 禁止副作用：<...>

### NEXT-002 — P1 — <标题>

<同上>

## 20.2 决策树 [REQUIRED when multiple plausible paths]

```text
先执行 <command/check>
├─ 若 A
│  ├─ 修改 <file::symbol>
│  └─ 运行 <test>
├─ 若 B
│  ├─ 验证 <hypothesis>
│  └─ 不要修改 <protected area>
└─ 若状态与交接不一致
   ├─ 停止写操作
   ├─ 记录差异
   └─ 根据当前仓库重新制定最小下一步
```

## 20.3 完成路径

```text
NEXT-001
  -> NEXT-002
  -> regression tests
  -> cleanup
  -> requirement audit
  -> final review
```

## 20.4 可并行项

| 任务 | 可与何项并行 | 冲突文件/资源 | 合并方式 |
|---|---|---|---|
| <...> | <...> | <...> | <...> |

# 21. 不要重复、不要破坏与受保护状态

## 21.1 不要重复的失败尝试 [REQUIRED]

- REJ-001：<简述及证据>
- <...>

## 21.2 不要覆盖的修改 [REQUIRED]

- `<path/range>`：<属于用户或其他任务；原因>
- <...>

## 21.3 禁止执行的操作

除非用户在接管后明确授权：

- 不要运行 `git reset --hard`、`git clean -fdx`、强制 checkout、rebase 或覆盖未提交修改。
- 不要擅自 commit、push、force-push、merge、删除分支或 stash 用户修改。
- 不要删除未知的 untracked/ignored 文件。
- 不要杀死未知进程、卸载未知 mount 或断开调试器。
- 不要更改工具链、依赖版本、架构边界或公共 API，只为绕过当前错误。
- 不要把未运行的测试描述为通过。
- 不要把 `[HYPOTHESIS]` 升级为 `[VERIFIED]`，除非有新证据。
- 不要在日志或交接中泄露秘密。

## 21.4 必须保持的不变量

- <...>

# 22. 开放问题

## 22.1 必须由用户决定

| ID | 问题 | 可选项 | 各选项影响 | 默认是否可安全采用 |
|---|---|---|---|---|
| Q-USER-001 | <...> | A/B | <...> | <...> |

## 22.2 可由接管者通过调查解决

| ID | 问题 | 最小调查步骤 | 预计影响 |
|---|---|---|---|
| Q-TECH-001 | <...> | <...> | <...> |

## 22.3 已回答但容易被误解的问题

| ID | 结论 | 来源 | 常见误解 |
|---|---|---|---|
| Q-CLOSED-001 | <...> | <...> | <...> |

# 23. 接管启动协议

## 23.1 接管者只读检查清单 [REQUIRED]

在第一次修改前必须完成：

- [ ] 找到并读取所有适用的 `AGENTS.md` / `AGENTS.override.md`。
- [ ] 完整读取本 `HANDOFF.md`，而不是只读摘要。
- [ ] 确认 CWD、repo root、workspace/worktree。
- [ ] 独立确认 branch、HEAD、base、upstream。
- [ ] 检查 staged、unstaged、untracked、required ignored 文件。
- [ ] 核对所有关键文件和 symbol 是否仍存在。
- [ ] 核对验证结果是否对应当前 HEAD 和 dirty 状态。
- [ ] 检查正在运行的 QEMU、调试器、容器、mount、端口。
- [ ] 列出本交接与当前实际状态的差异。
- [ ] 确认第一项 NEXT 操作仍然合理。
- [ ] 未经授权不执行破坏性 Git 或系统操作。

## 23.2 接管差异报告格式

```markdown
## Import discrepancy report

| ID | Handoff says | Actual state | Severity | Impact | Action |
|---|---|---|---|---|---|
| DIFF-001 | <...> | <...> | info/warn/blocking | <...> | <...> |
```

## 23.3 新对话最小启动消息

```text
接管任务 `<task-id>`。

交接文件：<path/to/HANDOFF.md>

先按其中“接管启动协议”完成只读核验，并报告任何差异。不要 reset、clean、
checkout、revert、stash、commit 或覆盖已有修改。核验完成后，直接从
NEXT-001 继续；仅当实际状态使 NEXT-001 不再成立时，先更新差异报告和交接文件。
```

# 24. 交接完整性审计

## 24.1 必填项检查 [REQUIRED]

- [ ] 用户目标、要求、纠正、非目标和禁止项已记录。
- [ ] 每项要求有稳定 ID、状态、实现位置和验证证据。
- [ ] branch、完整 HEAD、base、dirty 状态已记录。
- [ ] staged、unstaged、untracked、required ignored 文件已记录。
- [ ] 用户已有修改与当前任务修改已区分。
- [ ] 修改文件和关键 symbol 已记录。
- [ ] 已完成、部分完成、未开始已区分。
- [ ] 设计决策和被否决方案已记录。
- [ ] 事实、观察、推断和假设已分开。
- [ ] 最小复现、关键日志、错误签名已记录。
- [ ] 所有构建和测试都有命令、时间、exit code、HEAD 和日志。
- [ ] 未运行测试已明确列出。
- [ ] 环境、工具链和恢复命令已记录。
- [ ] 非 Git 状态、进程、端口、mount、QEMU/GDB 现场已记录或标记 N/A。
- [ ] 产物有路径、生成方式和校验值。
- [ ] 下一步含精确命令、成功/失败分支和停止条件。
- [ ] 不要重复/不要破坏的内容已记录。
- [ ] 没有秘密或凭据值。
- [ ] 本文件已在写完后重新读取并与当前状态比较。

## 24.2 缺失信息报告 [REQUIRED]

| 缺失项 | 原因 | 影响 | 接管者如何补全 |
|---|---|---|---|
| <None 或具体内容> | <...> | <...> | <...> |

## 24.3 新鲜度与可信度

- Handoff 状态：`READY / PARTIAL / BLOCKED`
- 整体可信度：`high / medium / low`
- 可能最先过期的字段：<进程/PID/分支/测试/外部服务等>
- 必须立即重新验证的内容：<...>
- 未解决的内部矛盾：<None 或列表>

## 24.4 最终自检结论 [REQUIRED]

```text
HANDOFF_READY=<yes|no>
REASON=<...>
FIRST_NEXT_ACTION=<...>
CURRENT_BRANCH=<...>
CURRENT_HEAD=<...>
WORKING_TREE=<clean|dirty|unknown>
BLOCKING_USER_DECISION=<none|...>
```

# 附录 A：命令与原始输出索引

| ID | 命令 | 输出文件 | Exit | 时间 | 备注 |
|---|---|---|---:|---|---|
| CMD-001 | <...> | `logs/<...>` | <...> | <...> | <...> |

# 附录 B：关键对话证据

只摘录会改变实施含义的短片段；其余使用对话回合或 session 指针，避免复制整段聊天。

| ID | 来源 | 摘要/短摘录 | 影响 |
|---|---|---|---|
| CHAT-001 | <turn/time> | <...> | <...> |

# 附录 C：术语与缩写

| 术语 | 含义 |
|---|---|
| <...> | <...> |

# 附录 D：机器可读摘要（可选）

```yaml
summary:
  goal: "<...>"
  status: "<...>"
  branch: "<...>"
  head: "<...>"
  dirty: true
  first_next_action: "<...>"
requirements:
  done: []
  partial: []
  todo: []
tests:
  passed: []
  failed: []
  not_run: []
blockers: []
protected_paths: []
```
