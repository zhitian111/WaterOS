# 重复任务总则

本目录定义面向整个 WaterOS 项目的重复任务。这些任务通常在完成一段编码后执行一次，用于把代码状态同步到文档、注释、导出结果和任务面板中。

## 共同要求

每个任务文档都必须说明：

- 任务目标
- **执行前必须参考的 prompt**：列出完整路径，不得只写文件名
- **执行前必须参考的导出文档**（仅编码类与分析类任务）：列出 `docs/exports/` 下的完整路径
- 需要优先查看的源文件
- 搜索范围
- 输出目录
- 并行拆分策略
- 完成后的回填要求

## 使用方式

向 Agent **只提供本目录下的任务文件**即可，例如 `@docs/tasks/commenting.md`。无需再单独 @ `docs/prompts` 或 `docs/exports`——任务文件内已写明 Agent 须自行打开阅读的 prompt 与导出文档路径；Agent 收到任务后应先按清单读取这些文件，再开始执行。

## 任务类型与必读材料

| 任务 | 类型 | prompt | 导出文档 (`docs/exports`) |
|------|------|--------|---------------------------|
| `commenting.md` | 编码类 | `general`、`structure`、`coding`、`documentation`、`architecture` | 需要 |
| `analyze_kernel_log.md` | 分析类 | `general`、`structure`、`architecture` | 需要（按失败子系统选读） |
| `run_testsuits_qemu.md` | 运行类 | `general`、`structure`、`coding` | 不需要 |
| `ltp_autonomous_iteration.md` | 自主迭代类 | `general`、`structure`、`coding`、`debug_workflow`；另读 `run_testsuits_qemu.md` | 按需 |
| `export_public_api.md` | 导出类 | `general`、`structure`、`documentation`、`architecture` | 不需要（本任务生成 `public-api/`） |
| `export_impl_guide.md` | 导出类 | 同上 | 不需要（本任务生成 `impl-guide/`） |
| `export_architecture.md` | 导出类 | 同上 | 不需要（本任务生成 `architecture/`） |
| `export_features.md` | 导出类 | 同上 | 不需要（本任务生成 `features/`） |
| `export_release_overview.md` | 导出类 | 同上 | 不需要（本任务生成 `release-overview/`） |
| `maintain_todolist.md` | 规划类 | 同上 | 不需要 |

prompt 与导出文档的完整路径索引见 `docs/prompts/README.md` 与 `docs/exports/README.md`。

## 一级组件导出路径模式

编码类或分析类任务涉及某一级组件时，按需阅读以下路径（将 `<component>` 替换为组件名，如 `wateros-syscall`）：

- `docs/exports/features/<component>.md`
- `docs/exports/public-api/<component>.md`
- `docs/exports/impl-guide/<component>.md`

全局视图：

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`

默认一级组件列表：`wateros-abi`、`wateros-base`、`wateros-driver`、`wateros-fs`、`wateros-ipc`、`wateros-klog`、`wateros-mm`、`wateros-platform`、`wateros-runtime`、`wateros-task`、`wateros-utils`、`wateros-vfs`。

## 并行执行原则

WaterOS 的大部分重复任务都可以按一级组件并行拆分。默认拆分单位包括：

- `wateros-abi`
- `wateros-base`
- `wateros-driver`
- `wateros-fs`
- `wateros-ipc`
- `wateros-mm`
- `wateros-platform`
- `wateros-runtime`
- `wateros-task`
- `wateros-utils`
- `wateros-vfs`

执行任务时应优先并行处理不同组件，再在组件内部按 API、聚合层和 impl 层细分。

## 任务索引

- `commenting.md`
- `export_public_api.md`
- `export_impl_guide.md`
- `export_architecture.md`
- `export_features.md`
- `maintain_todolist.md`
- `export_release_overview.md`
- `run_testsuits_qemu.md`
- `analyze_kernel_log.md`
- `ltp_autonomous_iteration.md`（**LTP AI 托管多轮迭代**；停止条件为用户主动中断）
