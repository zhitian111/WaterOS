# 项目架构 Prompt

WaterOS 的核心设计不是单体 crate，而是基于组件、API、实现和 feature 的分层组织。

## 总体组织思路

- 根 crate `wateros` 聚合一级组件。
- 一级组件再继续拆分 API 子包、功能子组件、impl 子包。
- 对外稳定接口由聚合 crate 导出，而不是由具体 impl 直接对外暴露。

## API 与 impl 设计范式

典型结构如下：

- `component/src/lib.rs`：聚合层，导出最终接口。
- `component/component-api/api-v0/`：API 契约层。
- `component/component-impl/impl-xxx/`：具体实现层。

设计要求：

- `api-v0` 用于定义稳定契约。
- `impl-*` 用于承载平台、算法或策略差异。
- 聚合层通过 feature 绑定实现并给出统一导出名。

## feature 选择模式

- 根 crate feature 决定一级组件使用哪个实现。
- 一级组件 feature 继续向下传递到子组件。
- 子组件再决定具体 impl crate。
- `os/feature-tree.txt` 是理解整条 feature 链的关键来源。

## 模块拆分原则

- 优先按能力边界拆分，而不是按实现细节拆分。
- API 层与 impl 层应保持一对多关系。
- 平台相关代码优先放在 platform、arch、firmware 与驱动实现中。
- 面向内核最终可见的接口，应汇聚到聚合 crate。

## 当前项目的明显特征

- `wateros-platform`、`wateros-driver`、`wateros-mm` 已经明显体现 API/impl/聚合范式。
- `wateros-fs`、`wateros-ipc`、`wateros-task` 等仍有部分骨架或占位实现。
- 根 `os/src/main.rs` 当前仍承担 bring-up、自检和启动流程编排职责。

## 编写或修改文档时的架构原则

- 描述 API 时，优先基于聚合层导出的实际接口。
- 描述新增 impl 的方法时，必须覆盖 `Cargo.toml`、feature、依赖、实现类型、导出链。
- 描述架构图时，至少区分根 crate、一级组件、API 契约层和 impl 层。
