# 重复任务总则

本目录定义面向整个 WaterOS 项目的重复任务。这些任务通常在完成一段编码后执行一次，用于把代码状态同步到文档、注释、导出结果和任务面板中。

## 共同要求

每个任务文档都必须说明：

- 任务目标
- 执行前需要参考的 prompt
- 需要优先查看的源文件
- 搜索范围
- 输出目录
- 并行拆分策略
- 完成后的回填要求

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
