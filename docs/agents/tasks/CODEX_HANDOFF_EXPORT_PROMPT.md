# Codex 可验证交接生成提示词（EXPORT）

将下面整段发送给即将结束工作的旧 Codex 对话。只需要替换参数区；未替换的参数由 Codex根据当前任务合理推断。

```text
[MODE]
EXPORT

[PARAMETERS]
TASK_ID=<稳定任务 ID；未指定时根据任务生成 kebab-case>
TASK_TITLE=<任务名称；未指定时从当前对话推断>
OUTPUT_DIR=docs/agent/handoffs/<TASK_ID>
HANDOFF_FILE=<OUTPUT_DIR>/HANDOFF.md
TEMPLATE_PATH=<可选；例如 .agents/skills/verified-handoff/assets/HANDOFF_TEMPLATE.md>
INCLUDE_KERNEL_QEMU_SECTION=auto
CREATE_SNAPSHOT_FILES=yes
RUN_SAFE_VALIDATION=yes
MAX_INLINE_LOG_LINES=120
```

你现在要执行一次“高完整度、可验证、可恢复的任务交接”。目标是：让一个完全没有读过本对话的新 Codex 对话，只依赖当前仓库、适用的 AGENTS 指令、交接目录和其中的证据，就能继续工作，而不会丢失用户要求、设计原因、当前进度、失败经验、运行现场或下一步。

这不是普通聊天总结。不要仅凭记忆写一段叙述；必须同时审计完整对话、当前仓库、Git 状态、相关文件、构建/测试结果和非 Git 运行状态。

## 一、总体行为约束

1. 暂停继续扩大实现。除非为了完成一个已在运行的安全检查、保存现场或验证当前状态，不要继续开发新功能。
2. 不得执行破坏性或改变历史的操作，包括但不限于：
   - `git reset --hard`
   - `git clean`
   - 强制 checkout
   - rebase
   - 覆盖未提交文件
   - 删除未知 untracked/ignored 文件
   - 未经授权 commit、stash、push、merge 或删除分支
   - 未经确认杀死未知进程、卸载 mount、解除 loop device
3. 不得把密码、token、私钥、Cookie、完整授权头、账号凭据或含秘密的配置内容写入交接。只记录变量名、用途和安全获取方式，值用 `<REDACTED>`。
4. 不得把未验证的推测写成事实；必须使用以下标签：
   `[USER-REQ]`、`[USER-CORRECTION]`、`[VERIFIED]`、`[OBSERVED]`、
   `[INFERRED]`、`[DECISION]`、`[HYPOTHESIS]`、`[TODO]`、
   `[BLOCKED]`、`[STALE]`、`[UNKNOWN]`、`[N/A]`。
5. 不适用的模板项写 `N/A — 原因`；无法获取的项写
   `UNKNOWN — 缺失原因 — 获取方法`。不要静默删除模板章节。
6. 时间使用 ISO 8601 并注明时区。
7. 长输出保存到 `<OUTPUT_DIR>/logs/`；补丁和状态快照保存到
   `<OUTPUT_DIR>/snapshots/`；产物索引保存到 `<OUTPUT_DIR>/artifacts/`。
   HANDOFF.md 中只保留关键摘录、路径、命令、exit code 和 SHA-256。
8. 交接文件应是“当前状态的索引和证据链”，不是把整个代码库、完整 diff
   或几万行日志重新粘贴进上下文。
9. 不要询问用户已经在本对话中回答过的问题。缺少非关键字段时应自行检查或标记
   UNKNOWN；只有确实需要用户决策且无法安全前进时才列入开放问题。
10. 若当前目录包含多个相关仓库，必须分别记录每个仓库的路径、分支、HEAD 和修改状态。

## 二、先审计完整对话

完整回顾当前对话，而不是只读取最近几轮。提取并使用稳定 ID 记录：

1. 用户最初目标、最终目标和实际使用场景。
2. 每一条功能要求、非功能要求、兼容要求、性能要求、输出格式要求。
3. 用户后续对要求的修改、收窄、扩展和优先级变化。
4. 用户对 Codex 先前理解的纠正。
5. 用户明确否决的方案、明确禁止的改动和必须保留的行为。
6. 用户已经作出的设计选择。
7. 尚未由用户决定但确实会影响实现的选择。
8. 用户偏好的工作方式，例如是否要求完整代码、最小改动、精确命令、验证证据等。
9. 对话中曾经声称“完成/通过/原因确定”的事项；这些事项必须根据当前仓库和证据重新分类为 VERIFIED、STALE 或 UNVERIFIED。
10. 对话中的失败尝试、关键日志、错误现象、假设和未完成计划。

对关键纠正和否定要求，保留简短的原始措辞摘要或准确的对话来源指针，避免在转述中改变含义。

## 三、发现并读取持久指令

1. 确认当前 CWD 和 Git 根目录。
2. 查找并完整读取当前作用域内实际会生效的：
   - 全局 `AGENTS.md` / `AGENTS.override.md`
   - 仓库根目录及从根到 CWD 路径上的 `AGENTS.md`
   - 更具体的 `AGENTS.override.md`
   - 项目要求的 README、PLANS、设计文档、测试说明
3. 在交接中记录这些文件的路径、适用范围、关键规则和当前状态。
4. 若规则冲突，明确记录最终采用哪条及原因。
5. 不要把本任务的临时进度错误地写进长期 AGENTS 指令，除非用户已要求更新。

## 四、采集仓库与 Git 事实

对每个相关仓库执行或获得等价信息。命令失败时记录命令、exit code 和错误，不要跳过。

```bash
pwd
git rev-parse --show-toplevel
git status --porcelain=v2 --branch
git rev-parse HEAD
git branch --show-current
git symbolic-ref --short -q HEAD
git remote -v
git log --oneline --decorate --graph -n 20
git diff --stat
git diff --name-status
git diff --cached --stat
git diff --cached --name-status
git diff --check
git ls-files --others --exclude-standard
git worktree list --porcelain
git stash list
git submodule status --recursive
```

还应尽力确定：

- upstream
- base branch
- 与 base 的 merge-base
- detached HEAD 状态
- staged、unstaged、untracked 的精确分类
- required-but-ignored 文件
- Git LFS 状态（若使用）
- 与任务相关的相邻仓库或子模块
- 哪些修改是用户已有、其他 agent 已有、当前任务产生或所有者不明

远程 URL 必须脱敏，移除其中可能包含的用户名、token 或密码。

### 快照文件

若 `CREATE_SNAPSHOT_FILES=yes` 且当前目录可安全写入：

1. 创建：
   - `<OUTPUT_DIR>/logs`
   - `<OUTPUT_DIR>/snapshots`
   - `<OUTPUT_DIR>/artifacts`
   - `<OUTPUT_DIR>/notes`
2. 保存 tracked working tree 补丁：
   ```bash
   git diff --binary HEAD -- . > <OUTPUT_DIR>/snapshots/working-tree.patch
   git diff --cached --binary -- . > <OUTPUT_DIR>/snapshots/staged.patch
   ```
3. 生成 untracked 文件清单，至少包含路径、大小、SHA-256、是否任务必需、是否疑似敏感。
   不要自动复制疑似秘密或大型数据。
4. 识别 required-but-ignored 文件，并记录其重建命令或迁移要求。
5. 对所有快照、日志和关键产物计算 SHA-256。
6. 若输出目录本身导致 `git status` 新增文件，要在最终 Git 快照中如实记录。

不要将补丁当作代码状态的唯一来源；仍需在 HANDOFF.md 中按文件和 symbol 解释改动的语义。

## 五、检查代码与项目结构

不要只根据 diff 文件名进行总结。实际打开并理解：

1. 所有已修改文件。
2. 与修改直接相关的调用者、被调用者、类型、trait、接口、测试和构建脚本。
3. 用户明确提及但尚未修改的关键文件。
4. 会决定行为的配置、feature、target spec、linker script、生成脚本和镜像脚本。
5. 修改前后的行为差异。
6. 当前实现的数据流、控制流、状态机、不变量、并发和错误路径。
7. 每处部分实现、TODO、占位符、临时调试代码和潜在死代码。
8. API/ABI/协议/磁盘格式/系统调用兼容性影响。
9. 哪些路径是受保护的用户修改，不能被接管者覆盖。

在“修改文件清单”中至少写出：

- 路径
- Git 状态
- 相关 symbol
- 修改目的
- 修改前行为
- 修改后行为
- 完成度
- 验证证据
- 风险或未覆盖边界

## 六、重建要求、决策和失败历史

1. 为要求建立稳定 ID 和追踪矩阵：
   - functional
   - non-functional
   - compatibility
   - performance
   - security/safety
   - documentation
   - prohibition
2. 每条要求必须关联：
   - 来源
   - 优先级
   - 状态
   - 实现位置
   - 验证证据
   - 剩余缺口
3. 为已采用设计建立决策记录，包括：
   - 背景
   - 候选方案
   - 最终选择
   - 选择原因
   - 代价
   - 影响范围
   - 允许重新考虑的触发条件
4. 为每个失败或被否决方案记录：
   - 实际尝试了什么
   - 命令或代码位置
   - 结果
   - 失败原因是否已确定
   - 证据
   - 哪些部分绝对不应重复
   - 何种新证据出现时才值得重试
5. 将事实、直接观察、推断和假设分开；每个假设写明最小验证实验和结果分支。

## 七、验证当前状态

若 `RUN_SAFE_VALIDATION=yes`：

1. 从 AGENTS、Makefile、README、CI、现有对话和脚本中找出项目真实使用的构建、测试、格式化、lint、运行和最小复现命令。
2. 优先运行：
   - 快速且与本任务最相关的构建
   - 最小复现
   - 修改对应的测试
   - 必要的回归测试
3. 不要为了“看起来完整”擅自运行会修改数据、部署、推送、长时间占用独占资源或需要高权限的命令。
4. 每次命令记录：
   - 完整命令
   - CWD
   - 环境/target/features
   - 开始时间
   - exit code
   - 关键输出
   - 完整日志路径
   - branch、HEAD 和当时 dirty 状态
5. 测试结果若来自旧 HEAD 或修改后的工作区，必须明确说明；不得笼统写“测试通过”。
6. 未运行的测试必须列出原因和风险。
7. 将要求与验证一一关联，说明哪些要求仍没有证据。
8. 对难以一次验证的任务，记录当前 baseline、每次迭代改了什么、指标变好或变坏以及下一次实验。

## 八、采集非 Git 现场

检查并记录与任务相关的：

- 正在运行的进程及完整命令
- QEMU、GDB、LLDB、tmux/screen 会话
- 端口、Unix socket
- 容器、虚拟机
- mount、loop device
- 临时目录、缓存、锁和 PID 文件
- 生成中的任务或被中断的命令
- 当前调试器停点、断点、backtrace、关键寄存器和变量
- 不能由 Git 迁移的内存状态
- 重建和安全清理方法

不要只记录 PID，因为 PID 会过期；必须同时记录完整重建命令和现场是否可恢复。

## 九、内核/QEMU/裸机任务专项要求

当项目涉及内核、Rust `no_std`、QEMU、RISC-V、LoongArch、系统调用、文件系统、驱动、页表或性能分析时，必须填写模板的专项章节，并尽力记录：

1. host/target architecture、Rust target、toolchain、components、features、profile。
2. linker script、kernel ELF、raw image、用户程序镜像及其 hash/build-id。
3. 完整启动链：firmware/OpenSBI/bootloader/kernel entry。
4. 完整 QEMU 命令及 version、machine、cpu、memory、smp、BIOS、devices、
   block image、serial、monitor、GDB、trace/plugin 参数。
5. DTB 来源、路径/hash、关键设备节点、MMIO 地址、IRQ。
6. 磁盘镜像生成命令、filesystem/features、mount/loop 状态、测试数据。
7. 异常现场：
   - architecture/hart/task
   - privilege mode
   - cause/scause
   - epc/sepc/era
   - tval/stval/badv
   - status/sstatus
   - satp/page-table translation
   - sp/ra
   - syscall number/args
   - faulting instruction
   - 最后正常日志和首个异常日志
8. GDB 启动和连接命令、symbol file、breakpoints、backtrace、registers。
9. 调度器、锁、中断、frame allocator、heap、页表、timer 和 syscall 状态。
10. 性能分析方法、采样命令、符号化情况、baseline、热点和虚拟化导致的限制。
11. 所有串口日志、trace、profile、反汇编和镜像的路径、生成命令及 SHA-256。

## 十、生成 HANDOFF.md

1. 若 `TEMPLATE_PATH` 存在，完整读取并严格按其结构填写。
2. 若不存在，仍应生成包含以下全部部分的 HANDOFF.md：
   - 交接契约和标签
   - 执行摘要
   - 任务来源与用户要求/纠正
   - 目标、范围、非目标、交付物、完成标准
   - 需求追踪矩阵
   - 已加载 AGENTS 和项目规范
   - 仓库/模块/symbol 地图
   - 环境与工具链
   - Git/worktree 精确快照
   - 文件与代码修改清单
   - 当前实现状态和行为模型
   - 决策记录与被否决方案
   - 事实/观察/推断/假设
   - 最小复现和调试状态
   - 构建/测试/验证矩阵
   - 产物、日志和数据索引
   - 进程、端口、mount、容器、QEMU/GDB 等临时现场
   - 外部依赖、权限和秘密边界
   - 工作日志
   - 风险、阻塞和技术债
   - 带命令和分支条件的下一步队列
   - 不要重复/不要破坏
   - 开放问题
   - 接管协议
   - 完整性审计
3. 所有关键结论都要有来源或证据。
4. 第一项 NEXT 操作必须具体到“命令”或“文件::symbol”，且包含：
   - 目的
   - 预期结果
   - A/B 分支
   - 失败时停止条件
5. 不要用“继续调试”“完善实现”“检查一下”这类无法执行的下一步。
6. 对用户已有修改、当前 agent 修改、其他 agent 修改和未知所有权修改进行区分。
7. `handoff_status=READY` 仅在接管者无需读取旧聊天即可理解并继续任务时使用。

## 十一、写完后的交叉核验

写完 HANDOFF.md 后必须重新读取它，并执行最终一致性检查：

1. HANDOFF 头部 branch/HEAD/dirty 是否与此刻实际状态一致。
2. 输出目录新增文件是否已反映到 Git 状态。
3. 每个已修改文件是否出现在修改清单。
4. 每条 MUST/MUST NOT 要求是否在追踪矩阵中。
5. 每个“已完成”是否至少有代码位置或验证证据。
6. 每个测试是否有命令、exit code、时间和对应 HEAD。
7. 每个未验证结论是否正确标为 HYPOTHESIS/UNKNOWN/STALE。
8. 下一步是否仍适用于当前状态。
9. 所有路径是否实际存在，或明确标记为外部/已丢失。
10. 快照和产物 SHA-256 是否正确。
11. 是否意外写入秘密。
12. 是否仍有内部矛盾或缺失项；若有，在完整性审计中明确列出。
13. 最终设置：
    - `HANDOFF_READY=yes/no`
    - `handoff_status=READY/PARTIAL/BLOCKED`
    - `FIRST_NEXT_ACTION`
    - `CURRENT_BRANCH`
    - `CURRENT_HEAD`
    - `WORKING_TREE`
    - `BLOCKING_USER_DECISION`

若核验发现不一致，先修正 HANDOFF.md，再输出最终回复。

## 十二、最终回复格式

最终只输出：

1. 交接文件路径。
2. handoff status、branch、HEAD、dirty 状态。
3. 已创建的日志/补丁/产物目录。
4. 未能收集或验证的关键内容。
5. 一段不超过 15 行、可直接粘贴到新对话的接管启动消息。

不要在最终回复中重新复制整份 HANDOFF.md。
