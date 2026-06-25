# 系统调用语义审计与收敛

## 任务目标

审计内核当前**已实现或已注册**的全部系统调用，以 **Linux syscall 语义为 baseline**，检查底层实现的可靠性与语义正确性；将未完整支持的路径**收敛到可控范围**（不支持则明确失败，而非静默错误或卡死）。

**背景假设**：跑测试时频繁出现意外卡死，部分原因是内核表面上支持的 syscall 语义，底层实现并不完整——许多路径在初期阶段只是 stub 或部分实现，却在未校验的情况下继续执行。

**本任务交付**：审计文档 +（可选）收敛代码修改。不要求一次实现全部缺失语义。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/architecture.md`
- `docs/prompts/coding.md`

## 执行前必须参考的导出文档

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/exports/impl-guide/wateros-syscall.md`
- `docs/exports/features/wateros-abi.md`
- 按 syscall 关联子系统按需阅读：`wateros-vfs`、`wateros-fs`、`wateros-task`、`wateros-ipc`、`wateros-mm`、`wateros-cred` 等对应 `docs/exports/features/<component>.md`

## 需要优先查看的源文件

| 文件/目录 | 用途 |
|-----------|------|
| `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs` | syscall 分发入口、`dispatch_unknown`、未实现 nr 处理 |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/` | 各 syscall 实现 |
| `os/components/wateros-abi/` | syscall 号、常量、用户态契约 |
| 平台 trap 路径（`wateros-platform` 等） | 用户态进入内核的入口 |
| `docs/exports/features/wateros-syscall.md` | 已有能力快照（对照基线，不替代代码审计） |

## 搜索范围

- `os/components/wateros-syscall/**`
- `os/components/wateros-abi/**`
- 各 syscall 实现所调用的 VFS、task、IPC、mm、cred 等下游模块
- 架构相关 syscall 分发与寄存器传参路径

## 输出目录

主 agent 汇总后写入（若目录不存在则创建）：

- `docs/audits/syscall-issues.md` — **文档 A**：《系统调用潜在问题清单》
- `docs/audits/syscall-coverage.md` — **文档 B**：《系统调用支持范围说明》
- `docs/audits/syscall/` — 各 subagent 单 syscall 文档（`docs/audits/syscall/<syscall-name>.md`）

可选：

- `docs/audits/syscall-inventory.md` — 《系统调用清单》（拆分子任务前产出）

## 并行拆分策略

**主 agent 职责**：盘点 → 并行派发 subagent → 收集 → 去重合并 → 产出文档 A/B。

1. **盘点**：从 syscall 表、分发入口枚举所有已实现/已注册的 syscall（名称、编号、入口函数、主要实现文件、初步复杂度）。
2. **拆分单位**：默认**每个 syscall 一个 subagent**；强相关的可合并（如 `stat` / `fstat` / `lstat`，`open` / `openat`）。
3. **每个 subagent 交付**（单 syscall 文档），至少包含：
   - syscall 名称、编号、入口与主要实现位置
   - **Linux 语义**：功能；主要参数、flag、option 的含义与组合效果
   - **当前实现覆盖范围**：已实现 / 部分实现 / stub / 未实现 的路径
   - **可靠性分析**：参数校验、错误码、边界条件、与 Linux 不一致之处
   - **潜在问题列表**（按严重程度：卡死 / 语义错误 / 错误码不符 / 未实现却继续执行 等）
   - **收敛建议**：哪些 flag/参数组合应暂时拒绝；建议的 warn 内容与应返回的错误值
4. **主 agent 汇总**为文档 A、文档 B；统一 warn 风格与错误码约定；附《高优先级收敛列表》（易导致卡死或语义严重偏差的 syscall 及参数组合）。

## 修复与收敛策略

对确认**语义未完整支持或实现不可靠**的 syscall 路径，不要按「看似支持」的方式强行执行：

1. 在入口处或分支处加入判断
2. 日志打印 **warn**，内容包含：**系统调用名称、系统调用号、系统调用参数**（及相关的 flag/option）
3. 返回与 Linux 语义一致的**明确错误值**（如 `-EINVAL`、`-ENOSYS` 等；具体选择写入文档并保持一致）
4. 将路径记入文档 A，标注为「已收敛 / 待实现」

具体修改位置、判断条件由 subagent 在单 syscall 文档中给出建议；主 agent 汇总时去重并统一实现风格。

## 约束与假设

- 以 **Linux syscall 语义**为对照 baseline（含常见 flag、错误码约定）
- 面向**所有**已注册/已实现的系统调用，不遗漏
- 优先标注会导致**卡死、无限等待、静默错误**的路径
- 与用户沟通使用**简体中文**

## 完成后的回填要求

- 将 `docs/audits/syscall-issues.md`、`docs/audits/syscall-coverage.md` 纳入版本管理
- 若本轮已落地收敛代码，在 `docs/exports/features/wateros-syscall.md` 中同步「已收敛 / 明确拒绝」的语义（或注明由审计文档承接，避免两份事实源冲突）
- 高优先级项可回填 `docs/roadmap/todolist.md` 或相关 work package

---

## 主 Agent 执行顺序

1. 扫描代码库，生成《系统调用清单》
2. 按清单并行启动 subagent
3. 收集各 subagent 单文档
4. 去重、合并，产出文档 A、文档 B
5. （可选）按优先级执行收敛代码修改
6. 完成回填

## 与锁机制审计任务的对应关系

| 锁机制任务 | 本任务 |
|------------|--------|
| 单核多线程 + 持锁闭环为 baseline | Linux syscall 语义为 baseline |
| 按带锁数据结构分 subagent | 按 syscall 分 subagent |
| 文档 A：锁机制潜在问题 | 文档 A：syscall 潜在问题 |
| 文档 B：预期语义 vs 当前实现 | 文档 B：Linux 语义 vs 当前覆盖 |
| 不可靠路径 warn + 安全失败 | 不支持语义 warn + 返回错误 |
