# WaterOS 文档规范

本文件描述 Markdown、注释、Rust 文档注释、commit 与 PR 的统一要求。

## Markdown

- 使用中文为主，保留必要的英文技术名词。
- 标题应体现职责或问题域，不用空泛命名。
- 文档开头优先写用途、范围和事实来源。
- 架构、功能和 API 说明优先按组件拆分。

## 代码注释

### 普通注释

要求：

- 用于解释目的、约束、临时性和边界。
- 避免复述代码字面含义。

案例：

```rust
// 向上取整，避免低频时被截断为 0 tick。
```

### Rust 文档注释

要求：

- `pub` 暴露项优先写 `///`。
- 模块整体说明可使用 `//!`。
- 注释应描述契约和行为，而不只是实现方式。

维护说明：注释任务应以 **`docs/tasks/commenting.md`** 为准，覆盖**全部子 crate 与可选 feature 路径**，而非仅一级聚合层。内核 **`os/components/**`**、**`os/src/`**、用户态 **`user/`** 等处的 **`///` / `//!`** 应随对外契约演进同步；若某次交付仍漏掉子目录，应视为任务未完成而非规范例外。变更 **`pub`** 时同步更新 **`docs/exports/`** 与本指南相关段落。

案例：

```rust
/// 在给定时间后设置下一次定时器中断。
pub fn set_timer_after_ms(ms: u64) -> PlatformTimerResult<()> {
    set_timer_after(Duration::from_millis(ms))
}
```

## Commit 规范

采用 Conventional Commits：

```text
<type>(<scope>): <subject>
```

类型：

- `feat`
- `fix`
- `refactor`
- `docs`
- `test`
- `chore`
- `api`

案例：

- `feat(fs): implement devfs root and device nodes`
- `fix(driver-block): handle zero-length read`
- `api(mm): refine address space trait`

## PR 规范

PR 标题与 commit 风格一致，描述至少包含：

- 变更类型
- 涉及组件
- 简要说明
- 与 API 的关系
- 测试方法
- 检查清单

## Mermaid 规范

- 节点 ID 不含空格。
- 总图只展示主关系。
- 细节图按组件展开。
- 当文档表达“当前快照”时，不应加入尚未实现的连接关系。
