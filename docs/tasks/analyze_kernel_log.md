# 分析内核运行日志中的测试失败

## 任务目标

对已保存的内核/QEMU 运行日志（尤其是 **trace 模式** 下生成的长日志）进行检索与判读，**分条列出所有失败项**，并为每一项整理可直接复用作修复 prompt 的定位信息（日志行范围、用户可见错误、关键 syscall 返回值、推测根因、修复方向）。

**本任务只负责「读日志 + 归类 + 输出结论」**，不在同一次对话里默认修内核（除非用户明确要求）。

与 `run_testsuits_qemu.md` 的分工：

| 任务 | 输入 | 输出 |
|------|------|------|
| `run_testsuits_qemu.md` | 改开关 → 跑 QEMU → 生成日志 | 阶段通过/失败摘要、路线图回填 |
| **本任务** | 已有 `os/log` 或 `/tmp/*.log` | 逐条失败清单 + 修复用 prompt 素材 |

## 执行前必须参考的 prompt

- `docs/prompts/general.md`（构建与运行、日志级别）
- `docs/prompts/structure.md`（组件与 syscall 模块位置）
- `docs/prompts/architecture.md`（API/impl 分层与模块边界）

## 执行前必须参考的导出文档

先读全局索引，再按失败项涉及的子系统选读对应组件文件：

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`

常见失败关联组件（按需）：

- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/public-api/<component>.md`
- `docs/exports/impl-guide/<component>.md`

若用户额外 @ 了具体导出文档，优先以其为准理解模块边界。

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/log` 或用户指定的日志路径 | **主分析对象**（通常不提交 git） |
| `os/scripts/parse_qemu_test_log.py` | 快速汇总各 `*_testcode.sh` 组结果（对 trace 长日志仍须配合 grep） |
| `os/src/user_bringup_busybox.rs` | 当前启用了哪些 `SCRIPT_PATHS`、对应阶段 |
| `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs` | PANIC 时 `unsupported: unknown nr=` 的 dispatch 行为 |
| `os/components/wateros-syscall/**` | syscall 号表、已实现/未实现槽位 |
| `docs/tasks/run_testsuits_qemu.md` | 各测例组的**通过判读标准**（busybox 不能只看 START/END） |
| `docs/roadmap/test-case-full-pass-plan.md` | 阶段依赖与已知环境限制 |

## 搜索范围

**日志内检索**（优先用关键字，避免通读全文）：

```bash
LOG=os/log   # 或用户指定路径

# 失败与异常
grep -nE 'fail|FAIL|fatal|Fatal|panic|PANIC|Assert Fatal' "$LOG"
grep -nE '\[WARN\]|\[ERROR\]|killing user task|PageFault' "$LOG"

# 测例脚本输出
grep -n 'testcase busybox' "$LOG"
grep -n '#### OS COMP TEST GROUP' "$LOG"
grep -nE 'Testing |========== END|Pass!|FAIL LTP CASE' "$LOG"

# syscall 失败返回值（负数 errno）
grep -nE '\[syscall\] nr=[0-9]+ ret=-[0-9]+' "$LOG"

# 用户态工具常见错误串
grep -nE "can't open|No such file|Function not implemented|Bad file descriptor" "$LOG"

# 内核 PANIC
grep -nE 'unsupported: unknown nr=|Panicked at' "$LOG"
```

**代码侧**（定位根因时）：按失败项涉及的子系统搜索 `os/components/`、`os/src/`。

## 输出格式（交付给用户的结论）

按**优先级**分节，每一失败项一条，建议包含：

1. **编号与标题**（测例名 + 测试组，如 `busybox-glibc: df`）
2. **日志定位**：文件路径 + **行号范围**（trace 日志用 grep 行号即可）
3. **现象**：用户可见输出（如 `df: /proc/mounts: No such file or directory`）
4. **关键 syscall / trap**（若有）：`nr=`、`ret=`（对照 errno）、PageFault 的 `pc`/`stval`/`task_id`
5. **推测根因**（一句话）
6. **修复方向 / 关联项**（如「先修 procfs，连带 ps/free/df」）
7. **glibc vs musl**：若仅一侧失败，必须注明（对比两组 `testcase … fail` 行）

额外汇总：

- 各 **COMP TEST GROUP** 是否出现 `END`（无 END 且 PANIC = 该组未完成）
- **致命项**（内核 PANIC）单独置顶
- 启动阶段 **WARN**（非 busybox 用例）可单列「环境/自检」节

### errno 速查（日志中 `ret=-N`）

| ret | errno | 常见含义 |
|-----|-------|----------|
| -2 | ENOENT | 路径不存在 |
| -9 | EBADF | 坏 fd |
| -10 | ECHILD | wait 无子进程 |
| -17 | EEXIST | 已存在 |
| -22 | EINVAL | 非法参数 |
| -25 | ENOTTY | ioctl 不支持 |
| -38 | ENOSYS | syscall 未实现 |

riscv64 Linux 常用 nr（PANIC 时查表，**以 Linux 官方表为准**）：

| nr | 名称 | 备注 |
|----|------|------|
| 48 | faccessat | which 等 |
| 56 | openat | 打开 /proc、/dev |
| 65 | semop | busybox od 等可能触发；当前未实现会 panic |
| 276 | renameat2 | mv 重命名 |
| 260 | wait4 | 子进程回收 |

## 并行拆分策略

| 维度 | 说明 |
|------|------|
| **按测试组** | glibc / musl / basic / lua / LTP 等分段 grep `COMP TEST GROUP START` |
| **按失败类型** | 一人查 PANIC+syscall，一人查 busybox fail，一人查 WARN/PageFault |
| **按子系统** | procfs 类、devfs 类、VFS rename、内存 fault 分开归纳后再合并去重 |

同一失败若在两组均出现，**合并为一条**，注明「glibc + musl 均 fail」。

## 标准执行流程

### 1. 确认日志来源

- 路径：默认 `os/log`，或用户 @ 的文件
- 确认对应运行配置：查看日志内 `[busybox-bringup] script_path`、`COMP TEST GROUP START` 名称
- 注意日志含 ANSI 色码时，grep 仍可用；必要时 `sed 's/\x1b\[[0-9;]*m//g'`

### 2. 粗筛

```bash
# 可选：脚本摘要（非 trace 专用，但可快速看组是否 END）
python3 os/scripts/parse_qemu_test_log.py "$LOG"
```

然后按上文「搜索范围」逐类 grep，建立失败候选列表。

### 3. 细读上下文

对每个候选 **fail/PANIC/WARN**：

- 向上读 30–80 行、向下读 10–30 行（trace 模式）
- 提取紧邻的用户态 stderr（常与 `[syscall] nr=64` write 交错）
- 记录触发该测例的最后几条 syscall

### 4. 归类与去重

- **基础设施类**：缺 `/proc`、`/dev/null`、`/dev/misc/rtc` → 合并说明影响面
- **连锁失败**：如 `mv` ENOSYS → `rmdir` ENOENT → 标注依赖关系
- **libc 差异**：同命令 glibc fail / musl pass → 标为回归/ABI 差异

### 5. 输出结论

生成结构化 Markdown 列表（见「输出格式」），便于用户逐条复制为修复 prompt。

## 各测例类型的日志特征

| 类型 | 失败关键字 | 注意 |
|------|------------|------|
| **busybox** | `testcase busybox <cmd> fail` | 必须逐条统计；仅有 START/END 无 testcase 行 = 循环未跑 |
| **basic** | `Assert Fatal`、`Testing xxx` 无 `END test_` | |
| **libctest** | 缺 `Pass!`、PageFault | |
| **LTP** | `FAIL LTP CASE … : 非0` | |
| **内核 PANIC** | `unsupported: unknown nr=` | 同次运行后续脚本无效 |
| **trap** | `LoadPageFault` / `StorePageFault` + `killing user task` | 记录 `pc`、`stval`、`task_id` |

## 完成后的回填要求

- 若结论改变阶段优先级：增量更新 `docs/roadmap/test-case-full-pass-plan.md`、`docs/roadmap/todolist.md`
- **不要**把 `os/log`、`/tmp/*.log` 提交进 git
- 本任务**不要求**改代码；若用户后续要求修复，应另开对话并引用本任务输出的单条 prompt

## 任务完成自检清单

- [ ] 已确认日志路径与启用的 `SCRIPT_PATHS` / 测试组
- [ ] 使用关键字检索，未试图通读整份 trace 日志
- [ ] 每条失败含**行号范围**与用户可见错误
- [ ] PANIC / PageFault 已记录 syscall nr 或 trap 字段
- [ ] glibc/musl 差异已标注
- [ ] 连锁失败已注明依赖关系
- [ ] 致命项与 busybox 用例 fail 已分节
- [ ] 未在同一次任务中擅自提交日志或修改内核（除非用户要求）

## 示例：交给 Agent 的一次性用户 prompt 模板

```
@docs/tasks/analyze_kernel_log.md

请查看内核运行日志，确认哪些测试失败了 @os/log

请分条列出，因为我需要逐个解决。对于每一项问题请写有助于定位的信息
（日志定位、错误情况、推测原因等），我要作为 prompt 来使用。

日志为 trace 模式，内容可能很长，请用 fail/fatal/error/warn/panic 及
syscall 失败返回值等关键字定位。
```

只需 @ 本任务文件；prompt 与导出文档路径见上文「执行前必须参考」小节，由 Agent 自行打开阅读。
