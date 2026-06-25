# 锁机制审计与收敛

## 任务目标

审计内核中带锁数据结构的锁机制正确性，检查持锁/释锁时机、应加锁路径是否覆盖、持锁与释锁是否成对闭环；将不可靠路径**收敛到可控范围**，避免静默卡死或越界行为。

**背景假设**：跑测试时频繁出现意外卡死，部分原因是内核内数据结构的锁机制不正确——包括持锁和释放锁的时机、数据结构本身是否加锁、错误路径上是否漏释锁等。

**本任务交付**：审计文档 +（可选）修复/收敛代码修改。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/architecture.md`
- `docs/prompts/coding.md`

## 执行前必须参考的导出文档

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`
- 按带锁数据结构所在子系统按需阅读对应 `docs/exports/features/<component>.md`、`docs/exports/impl-guide/<component>.md`

常见关联组件：`wateros-task`、`wateros-vfs`、`wateros-ipc`、`wateros-mm`、`wateros-runtime`、`wateros-base`

## 需要优先查看的源文件

| 文件/目录 | 用途 |
|-----------|------|
| `os/components/wateros-base/**` | 基础同步原语（mutex、spinlock 等，若在此定义） |
| `os/components/wateros-task/**` | 任务/调度相关带锁结构 |
| `os/components/wateros-vfs/**` | VFS、fd 表、inode 等 |
| `os/components/wateros-ipc/**` | futex、pipe、信号等 |
| `os/components/wateros-mm/**` | 内存管理相关锁 |
| `os/components/wateros-runtime/**` | 运行时同步结构 |

具体路径以代码搜索为准（`lock`、`unlock`、`Mutex`、`Spinlock`、`RwLock` 及项目自定义锁类型）。

## 搜索范围

- `os/components/**` 内所有带锁数据结构及其调用方
- 平台相关代码中若存在 per-CPU 或 arch 锁，纳入审计但单独标注

## 输出目录

主 agent 汇总后写入（若目录不存在则创建）：

- `docs/audits/lock-issues.md` — **文档 A**：《锁机制潜在问题清单》
- `docs/audits/lock-coverage.md` — **文档 B**：《带锁数据结构支持范围说明》
- `docs/audits/locks/` — 各 subagent 单数据结构文档（`docs/audits/locks/<struct-name>.md`）

可选：

- `docs/audits/lock-inventory.md` — 《带锁数据结构清单》（拆分子任务前产出）

## 并行拆分策略

**主 agent 职责**：盘点 → 并行派发 subagent → 收集 → 去重合并 → 产出文档 A/B。

1. **盘点**：枚举所有带锁数据结构（名称、所在文件、锁类型、预估复杂度）；按**数据结构**拆分，而非按文件拆分。
2. **拆分单位**：默认**每个（或每组强相关的）带锁数据结构一个 subagent**。
3. **每个 subagent 交付**（单数据结构文档），至少包含：
   - 数据结构名称、所在文件、锁类型
   - 所有 `lock` / `unlock`（及等价 API）调用点
   - 调用链 / 持锁区间分析：漏释锁、重复释锁、持锁期间睡眠或调度、锁顺序不一致等
   - 潜在问题列表（按严重程度：死锁 / 卡死 / 数据竞争 / 语义偏差）
   - 当前实际支持的使用范围（哪些路径已正确加锁，哪些未覆盖）
   - **收敛建议**（若适用）：不可靠路径应如何 warn 与安全失败
4. **主 agent 汇总**为文档 A、文档 B；附《高优先级修复列表》（易导致死锁/卡死的结构及路径）。

## 检查基准与约束

- 当前阶段为**单核多线程**，审计与修复以该模型为 **baseline**
- 多核多线程相关实现若本身合理，**不视为错误**；但与单核语义冲突或引入额外死锁风险时须单独标注
- 关注**持锁闭环**：每一次持锁都应有可到达的释锁路径（含错误路径、提前返回）
- 不强行要求一次修完所有问题；优先标注会导致**卡死 / 死锁**的路径
- 面向**所有**已识别的带锁数据结构，不遗漏
- 与用户沟通使用**简体中文**

## 修复与收敛策略

对确认**锁语义未完整支持或实现不可靠**的路径，不要强行按「看似支持」的方式执行：

1. 加入判断
2. 日志打印 **warn**，内容包含：**数据结构名称、锁操作类型、调用位置（函数/文件）、相关上下文参数**（如有）
3. 返回明确错误值或走安全失败路径
4. 将路径记入文档 A，标注为「已收敛 / 待实现」

具体 warn 宏、错误码、修改位置由 subagent 在单数据结构文档中给出建议；主 agent 汇总时去重并统一风格。

## 完成后的回填要求

- 将 `docs/audits/lock-issues.md`、`docs/audits/lock-coverage.md` 纳入版本管理
- 若本轮已修复或收敛，在相关组件的 `docs/exports/features/<component>.md` 或 impl-guide 中简要注明（或指向审计文档）
- 高优先级项可回填 `docs/roadmap/todolist.md`

---

## 主 Agent 执行顺序

1. 扫描代码库，生成《带锁数据结构清单》
2. 按清单并行启动 subagent（可合并强耦合的小结构）
3. 收集各 subagent 单文档
4. 去重、合并，产出文档 A、文档 B
5. （可选）按优先级执行修复/收敛代码修改
6. 完成回填

## 与系统调用审计任务的对应关系

| 本任务 | 系统调用任务 |
|--------|----------------|
| 单核多线程 + 持锁闭环为 baseline | Linux syscall 语义为 baseline |
| 按带锁数据结构分 subagent | 按 syscall 分 subagent |
| 文档 A：锁机制潜在问题 | 文档 A：syscall 潜在问题 |
| 文档 B：预期语义 vs 当前实现 | 文档 B：Linux 语义 vs 当前覆盖 |
| 不可靠路径 warn + 安全失败 | 不支持语义 warn + 返回错误 |
