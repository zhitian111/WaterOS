# 导出当前架构图

## 任务目标

导出组件结构、模块结构以及 API/impl 的连接关系，可使用 Mermaid 表达。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/documentation.md`
- `docs/prompts/architecture.md`

本任务为导出类，输出写入 `docs/exports/architecture/`，**不需要**预先阅读现有导出结果（除非做增量对比）。

## 需要优先查看的源文件

- `os/Cargo.toml`
- `os/feature-tree.txt`
- 各一级组件 `Cargo.toml`
- 各一级组件聚合 `src/lib.rs`

## 搜索范围

- `os/components/**`
- `os/src/`
- `user/src/`，若任务需要用户态验证视角
- 旧版 `docs/*.md`，若需要迁移既有文档内容

## 输出目录

`docs/exports/architecture/`。

## 并行拆分策略

- 先按一级组件并行。
- 再按聚合层、API 层、impl 层拆分。
- 某组件仍处于骨架阶段时，应先导出当前状态，再单独标注缺口。

## 完成后的回填要求

- 如结果影响系统快照，更新 `docs/architecture/snapshot.md`。
- 如结果影响阶段目标，更新 `docs/roadmap/todolist.md`。
- 如结果影响人为协作认知，更新 `docs/guides/` 对应文件。
