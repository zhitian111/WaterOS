# 添加与格式化注释

## 任务目标

为**仓库内全部可维护源码**补齐必要注释、清理格式不一致的注释、补全对外可见 API 的 Rust 文档注释（`///` / `//!`），并在复杂或跨语言边界处用普通注释（`//` / 汇编侧 `#`）写清不变量与假设。

**不应**将范围缩小为「仅一级聚合 crate」「仅默认 feature 打开的 crate」「仅当前根 `wateros` 依赖路径」。占位 **`impl-dummy`**、可选 feature 专用 impl、仅被测试或工具链引用的子包，与主线实现**同等**需要注释。

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/documentation.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的导出文档

先读全局索引，再按当前注释范围选读对应一级组件文件：

- `docs/exports/README.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`
- `docs/exports/features/<component>.md`（按组件，如 `wateros-syscall`）
- `docs/exports/public-api/<component>.md`
- `docs/exports/impl-guide/<component>.md`

一级组件名列表见 `docs/tasks/README.md`「一级组件导出路径模式」。

## 需要优先查看的源文件（用于理解接线，不表示注释只写在这些文件）

- `os/Cargo.toml`
- `os/feature-tree.txt`
- 各一级组件 `Cargo.toml`
- 各一级组件聚合 `src/lib.rs`

## 搜索范围（全部都要扫到）

- **`os/components/**`**：任意深度的子 crate（`api-v0`、`impl-*`、中间聚合、无 feature 的叶子包等），**不**按是否被默认 feature 选中而跳过。
- **`os/src/`**（根内核 bin / 自检等）。
- **`user/**`**（用户态库、各 `bin`、共享模块、`build.rs`）：凡有逻辑或对外约定处，应有与 `documentation.md` 一致的注释策略。
- 仓库内其它参与构建的 **`build.rs`**、**`.S` / `.asm`**、链接脚本等：使用对应语言的注释语法写清与 Rust/ABI 的对应关系。
- 旧版 **`docs/*.md`**：仅在需要迁移既有说明进代码或导出文档时查阅。

## 注释粒度约定

- **`pub`**（含 `pub(crate)` 中对外部子系统有意义的项）：优先 **`///`**，写语义契约与错误/前置条件。
- **模块 / 多职责单文件**：文件顶部 **`//!`** 说明职责、与上下层（汇编、其它 crate）的边界。
- **完整度（重要函数与重要变量）**：凡**对内/对外重要**的 `fn`（含 `pub`、实现关键路径的私有函数）均应有说明——`pub` 侧用 `///`，其余可用 `//` 在函数上方一行写清用途与约束。承载协议、状态机或跨模块契约的 **`struct` 字段**、含 ABI/魔数含义的 **`static` / `const`**、以及函数内**跨多行仍须保持不变量**的局部绑定，应加注释；显而易见的循环索引、临时缓冲等可省略。
- **普通注释 `//` / 汇编 `#` 的风格**：**不必**按 `///`、`//!` 的 rustdoc 句式或固定章节来写；以**意图、不变量、平台假设、与上下层边界**为主，**简短、必要**即可。
- **函数体与复杂控制流**：分支多、与硬件/页表/陷阱上下文/跨语言边界相关的逻辑，在**阶段边界**处用 **`//`** 标出「当前在做什么、依赖何种假设」，避免只给聚合层写文档而实现文件无说明；私有类型与 `repr(C)`、寄存器约定、页大小等处同样用 **`//`** 写清。

## 输出目录

`docs/exports/features/` 与必要时更新 `docs/guides/documentation.md`。

## 并行拆分策略

- 可按一级组件并行，但**每个子 agent 必须遍历该组件下全部子 crate 与全部 `src` 文件**，不得只处理聚合 `src/lib.rs`。
- 再按目录拆分时，以「目录树」或「crate 列表」为单元并行，并显式列出**可选 feature / dummy impl** 路径以免遗漏。
- 某组件仍处于骨架阶段时，应先导出当前状态，再单独标注缺口；占位实现同样要有「当前行为 / 后续替换点」说明。

## 完成后的回填要求

- 如结果影响系统快照，更新 `docs/architecture/snapshot.md`。
- 如结果影响阶段目标，更新 `docs/roadmap/todolist.md`。
- 如结果影响人为协作认知，更新 `docs/guides/` 对应文件。
