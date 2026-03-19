# WaterOS 协作开发工作流

本文档定义 WaterOS 多人协作的开发流程，包括角色分工、分支策略、Commit/PR 规范、Review 流程与任务管理，便于充分发挥每人能力并保持主线稳定。

---

## 一、角色与职责

### 1. 核心开发（2 人）

- **职责**：组件 API 设计与主线架构
  - 在各自负责的组件下，定义 **原语级 / 元功能** 的 API（即 `*-api/api-v0` 中的 trait、类型、常量等）
  - 维护根 `Cargo.toml`、feature 树、组件边界与依赖关系
  - 评审实现侧 PR，决定是否合并
- **产出**：API 设计文档、接口代码（如 `wateros-fs-api-v0`）、以及必要时在 `impl-dummy` 中的占位实现或示例

### 2. 实现开发（其他成员）

- **职责**：在既定 API 下做具体实现
  - 在对应组件的 `*-impl/impl-xxx` 中实现 `api-v0` 定义的接口（不修改 API 定义本身，除非经核心开发同意）
  - 为新实现添加 feature、在根/组件 `Cargo.toml` 中挂接
  - 通过 PR 交付，并配合 Review 修改

**原则**：API 由核心开发统一设计，实现开发专注「在 API 约束下把功能做对、做稳」。

---

## 二、分支策略

### 1. 常设分支

| 分支 | 说明 | 谁可推送 |
|------|------|----------|
| `main` | 稳定可构建、可运行的主线；所有合并均通过 PR | 仅通过 PR 合并，不直接 push |
| `develop`（可选） | 集成开发分支，未完全稳定的改动先合到这里，再定期合到 main | 核心开发 / 约定成员 |

若团队规模小、希望流程简单，可以**仅保留 `main`**，所有 PR 直接指向 `main`。

### 2. 开发用分支（实现开发 / 功能开发）

- **命名**：`impl/<组件>-<实现名>` 或 `feat/<组件>-<简短描述>`
- **示例**：
  - `impl/fs-devfs`：为 wateros-fs 做 devfs 实现
  - `impl/driver-block-virtio`：为 block 驱动做 virtio 实现
  - `feat/ipc-pipe-real`：ipc-pipe 的真实实现
- **从何拉取**：从 `main`（或 `develop`，若启用）拉取，保证基于最新主线开发。
- **生命周期**：对应功能合并到 main 后即可删除该分支。

### 3. 操作步骤小结（实现开发）

1. 从最新主线拉取并创建分支：
   ```bash
   git fetch origin
   git checkout main
   git pull origin main
   git checkout -b impl/fs-devfs
   ```
2. 在 `os/components/wateros-fs/fs-impl/impl-devfs/` 等目录开发，遵循现有 API。
3. 本地测试通过后 push 并创建 PR：
   ```bash
   git push origin impl/fs-devfs
   ```
4. 在 GitHub/GitLab 上创建 PR，目标分支为 `main`（或 `develop`），填写 PR 模板。
5. 核心开发 Review 通过后合并；合并后删除分支（或由维护者统一清理）。

---

## 三、Commit 规范

采用 **Conventional Commits** 风格，便于自动生成 Changelog 和按类型过滤历史。

### 1. 格式

```
<type>(<scope>): <subject>

[optional body]

[optional footer]
```

- **type**：必填，见下表。
- **scope**：可选，建议用组件/子模块名（如 `fs`, `driver-block`, `ipc-pipe`）。
- **subject**：必填，简短祈使句，首字母小写，结尾不加句号。

### 2. type 取值

| type | 说明 | 示例 |
|------|------|------|
| `feat` | 新功能（含新 API、新 impl） | `feat(fs): add devfs mount API` |
| `fix` | 修复 bug | `fix(driver-block): handle zero-length read` |
| `refactor` | 重构（不改变行为） | `refactor(vfs): simplify inode cache` |
| `docs` | 仅文档 | `docs(api): document pipe semantics` |
| `test` | 测试相关 | `test(ipc): add pipe integration test` |
| `chore` | 构建/脚本/配置等 | `chore: bump rust toolchain in Makefile` |
| `api` | **仅核心开发**：API 定义变更 | `api(fs): add read_at/write_at to FileOps` |

实现开发在实现层提交用 `feat(scope)` 或 `fix(scope)` 即可；涉及 API 定义时由核心开发用 `api(scope)` 提交。

### 3. 示例

```text
feat(fs): implement devfs root and device nodes

- add root inode and device registration
- wire to wateros-fs-api-v0
```

```text
fix(ipc-pipe): correct buffer boundary in blocking read
```

```text
api(driver-block): add sector_alignment to BlockDevice
```

---

## 四、PR 规范

### 1. 标题

- 与 commit 风格一致：`<type>(<scope>): <简短描述>`
- 示例：`feat(fs): implement devfs`、`fix(ipc-pipe): fix blocking read`

### 2. 描述（使用仓库提供的 PR 模板，见下）

必填项：

- **变更类型**：API 设计 / 新实现 / Bug 修复 / 重构 / 文档 / 其他
- **涉及组件**：如 wateros-fs, wateros-ipc-pipe
- **简要说明**：做了什么、为什么
- **与 API 的关系**：若为「新实现」，写明实现的 trait/API 版本（如 api-v0）；若修改了 API，需说明并标注需核心开发重点 Review
- **测试**：如何验证（qemu 跑测、单元测试、手动步骤等）
- **检查清单**：勾选「基于最新 main」「通过 format/lint」「无额外警告」等

### 3. 粒度建议

- 一个 PR 聚焦**一个组件的一个实现或一个逻辑变更**，便于 Review 和回滚。
- 若一大块功能必须拆成多个 PR，在描述中注明「Part 1/N」并列出依赖关系。

### 4. Review 与合并

- **谁 Review**：至少一名核心开发 Review 通过后再合并。
- **合并方式**：建议使用 **Squash and merge** 或 **Create a merge commit**，由仓库设置统一；合并后删除源分支。
- **冲突**：合并前若有冲突，由 PR 作者在分支上 rebase/merge 最新 main 并解决冲突后再合并。

---

## 五、任务管理（共同 TODO 列表）

目标：有一份**所有人可见、可认领、可跟踪**的任务列表，与分支/PR 对应。

### 1. 推荐方式（任选其一或组合）

- **GitHub Issues + Projects**  
  - 每个实现/功能开一个 Issue，用 label 区分：`api-design`、`impl`、`component:fs`、`component:driver` 等。  
  - 用 Project 看板管理：Backlog → In Progress → In Review → Done。  
  - 实现开发在分支/PR 里用 `Closes #123` 关联 Issue，合并后自动关闭。

- **GitLab Issues + Board**  
  - 同理：Issue 作为任务，Board 列：待办 / 进行中 / Review / 已完成。

- **独立看板（如 Trello / 飞书/钉钉 项目）**  
  - 卡片 = 任务，描述里写：组件、API 版本、对应分支名、负责人、PR 链接。  
  - 与仓库的约定：PR 标题或描述中写上「任务 ID」或链接，便于追溯。

### 2. 任务条目建议字段

- **标题**：简短描述（如「wateros-fs impl-devfs 实现」）
- **类型**：API 设计 / 实现 / Bug 修复 / 文档
- **组件**：wateros-fs、wateros-driver-block、wateros-ipc-pipe 等
- **负责人**：认领人
- **状态**：待办 / 进行中 / Review 中 / 已完成
- **关联**：分支名、PR 链接、设计文档链接（若有）
- **备注**：依赖的 API 或其它任务

### 3. 与工作流的对应

- 核心开发：创建「API 设计」类任务，并拆出「实现 xxx」子任务供实现开发认领。
- 实现开发：认领任务 → 创建对应分支 → 开发 → 提 PR → 在任务中填上 PR 链接，Review 通过后标记完成。

---

## 六、流程串联示例

1. **核心开发**在任务列表中创建「wateros-fs devfs 实现」，并注明依赖 `wateros-fs-api-v0`。
2. **实现开发 A** 认领该任务，从 main 拉取 `impl/fs-devfs`，在 `fs-impl/impl-devfs` 中实现，本地测试通过。
3. A 按规范写 commit，push 后提 PR，标题：`feat(fs): implement devfs`，描述中填模板、写清实现的 API 与测试方式，并在任务中贴上 PR 链接。
4. **核心开发** Review：确认只改实现层、符合 api-v0、无破坏性改动，提出修改意见。
5. A 修改后 push，核心开发通过后合并到 main，关闭对应 Issue/任务卡片。
6. 后续其他人从 main 拉新分支时，自然包含 devfs 实现。

---

## 七、文档与约定位置

| 内容 | 路径 |
|------|------|
| 本工作流 | `docs/WORKFLOW.md` |
| Commit 规范速查 | `docs/COMMIT_CONVENTION.md` |
| PR 模板 | `.github/PULL_REQUEST_TEMPLATE.md` |
| 任务列表示例与模板 | `docs/TASKS.md` |

任务管理推荐：GitHub/GitLab Issues + Projects（或团队约定的看板），在 README 中放链接便于全员访问。

按上述流程，可以稳定地「核心设计 API、多人并行实现」，并保持主线清晰、可追溯。
