# Codex 交接接管提示词（IMPORT）

将下面整段发送给新的 Codex 对话，并替换 `HANDOFF_FILE`。

```text
[MODE]
IMPORT

[PARAMETERS]
HANDOFF_FILE=docs/agent/handoffs/<TASK_ID>/HANDOFF.md
CONTINUE_AFTER_AUDIT=yes
UPDATE_HANDOFF_DURING_WORK=yes
```

这是一个从其他 Codex 对话接续的任务。你的首要职责不是立刻修改代码，而是验证交接是否仍与当前仓库和运行环境一致，然后在不破坏已有工作的前提下无缝继续。

## 一、禁止事项

在完成只读接管审计前，不得：

- 修改源代码、配置或生成文件；
- 执行 `git reset`、`git clean`、checkout 覆盖、rebase、revert；
- stash、commit、push、merge 或删除分支；
- 删除 untracked/ignored 文件；
- 杀死未知进程、停止 QEMU、断开调试器、卸载 mount 或 loop device；
- 安装/升级依赖、改变工具链或切换架构；
- 把交接中的假设当作事实；
- 重新实现交接中已完成的部分。

除非发现阻塞性矛盾，不要重复询问用户已经在交接中回答过的问题。

## 二、读取完整上下文

1. 确认当前 CWD、仓库根目录、工作区类型和相关仓库。
2. 查找并完整读取当前作用域内会生效的所有：
   - `AGENTS.md`
   - `AGENTS.override.md`
   - 项目规范、README、PLANS、设计和测试文档
3. 完整读取 `HANDOFF_FILE`，不能只读执行摘要。
4. 打开交接中列出的：
   - 修改文件
   - 关键 symbol
   - 相关测试
   - 构建脚本
   - 日志、补丁和产物索引
5. 对长日志优先读取交接引用的关键区段；只有证据不足时再扩展读取。

## 三、独立核验当前状态

至少获取并比较：

```bash
pwd
git rev-parse --show-toplevel
git status --porcelain=v2 --branch
git rev-parse HEAD
git branch --show-current
git symbolic-ref --short -q HEAD
git log --oneline --decorate --graph -n 20
git diff --stat
git diff --name-status
git diff --cached --stat
git diff --cached --name-status
git ls-files --others --exclude-standard
git worktree list --porcelain
git stash list
git submodule status --recursive
```

同时检查：

- branch、HEAD、base、upstream、detached 状态；
- staged、unstaged、untracked、required ignored 文件；
- 交接目录是否存在；
- 补丁、日志、产物路径和 SHA-256；
- 用户已有修改与当前任务修改的边界；
- 交接引用的文件和 symbol 是否仍存在；
- 测试结果是否对应当前 HEAD 和 dirty 状态；
- 相关进程、QEMU、GDB、容器、端口、mount、loop 和临时目录；
- 工具链、target、依赖和环境变量是否发生变化。

不要因为某个 PID 已变化就直接判定交接无效；比较完整命令、作用和可重建方式。

## 四、生成接管差异报告

在任何写操作前，输出：

```markdown
## Import discrepancy report

| ID | Handoff says | Actual state | Severity | Impact | Action |
|---|---|---|---|---|---|
| DIFF-001 | ... | ... | info/warn/blocking | ... | ... |
```

差异至少覆盖：

1. 路径/CWD/worktree。
2. branch、HEAD、base、dirty。
3. staged、unstaged、untracked 和 ignored 文件。
4. 文件、symbol、接口或测试是否变动。
5. 工具链和环境。
6. 进程和调试现场。
7. 交接所引用的日志/产物是否缺失或 hash 不符。
8. 交接中 `[VERIFIED]` 结论是否已过期。
9. NEXT-001 是否仍然成立。

若无差异，明确写 `No material discrepancies found`，不要省略审计。

差异严重度：

- `info`：不影响继续，例如 PID 变化但可重建。
- `warn`：需要更新交接或重新验证，但仍可安全继续。
- `blocking`：继续可能覆盖工作、基于错误分支修改、破坏数据或实现错误目标。

## 五、复述接管理解

用简洁但完整的内容复述：

1. 最终目标。
2. MUST 与 MUST NOT 要求。
3. 完成标准。
4. 已完成、部分完成、未开始。
5. 当前 verified facts。
6. 尚未验证的 hypotheses。
7. 当前 blocker 和风险。
8. 用户明确决策。
9. 不能重复的失败方案。
10. 第一项具体下一步。

此复述用于证明你理解了交接，不是重写整份 HANDOFF。

## 六、处理不一致

### 无阻塞性差异

1. 将非阻塞差异更新到 HANDOFF。
2. 对已过期的关键结论重新验证。
3. 直接执行交接中的 NEXT-001。

### 有阻塞性差异

1. 停止写操作。
2. 不要 reset、checkout、clean 或覆盖来“恢复到交接状态”。
3. 保护当前所有修改和现场。
4. 根据当前仓库事实重新构造最小可继续路径。
5. 在 HANDOFF 中记录：
   - 原交接状态
   - 当前实际状态
   - 差异来源
   - 受影响的要求和验证
   - 新的 NEXT-001
6. 只有确实涉及用户意图或不可逆选择时才请求用户决定；纯技术差异应先自行调查。

## 七、继续工作

当 `CONTINUE_AFTER_AUDIT=yes`：

1. 从仍然有效的最高优先级 NEXT 项开始。
2. 不要重新实现已完成部分，除非验证证明其错误。
3. 每次重大修改都关联 Requirement ID、Decision ID 或 Hypothesis ID。
4. 每次运行命令记录 CWD、环境、exit code、时间、HEAD 和 dirty 状态。
5. 对假设采用最小实验；不要一次修改多个无关因素。
6. 保护用户已有修改和无关工作。
7. 达到以下任一节点时更新 HANDOFF：
   - 完成一项需求
   - 采用新设计决策
   - 否决一个方案
   - 发现新 blocker
   - Git branch/HEAD/dirty 状态显著变化
   - 测试状态变化
   - 运行现场不可恢复
   - 即将再次换对话
8. 更新时保留历史决策和失败尝试，不要用新状态覆盖掉重要原因链。

## 八、内核/QEMU专项接管

若交接涉及内核、Rust `no_std`、RISC-V、LoongArch 或 QEMU：

1. 在修改前核对完整 QEMU 命令、firmware、kernel ELF/image、disk image 和 DTB hash。
2. 检查 target/toolchain/linker script/features 是否一致。
3. 核对串口日志中的最后正常点和首个异常点。
4. 重新确认 trap/异常寄存器、hart/task/syscall 和 faulting instruction 是否对应当前构建。
5. 核对 GDB 使用的 symbol file 与正在运行的 kernel 是否同一 build。
6. 若 QEMU/GDB 现场已丢失，按交接记录的命令重建，不要假装现场仍存在。
7. 检查 mount/loop device，避免重复挂载或覆盖镜像。
8. 性能结果只有在 QEMU 参数、构建 profile、采样方法和 workload 一致时才可比较。

## 九、首次回复格式

首次回复应包含：

1. `Import discrepancy report`。
2. 你对目标、完成标准、当前进度和风险的复述。
3. 当前 branch、HEAD、dirty 状态。
4. 交接可信度：high/medium/low。
5. 即将执行的 NEXT-001 及其目的。

若不存在阻塞性差异且 `CONTINUE_AFTER_AUDIT=yes`，完成上述报告后直接继续执行，不要停下来要求用户再次确认。
