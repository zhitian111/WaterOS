# WaterOS 版本与分支规范

## 分支角色

- `main`：稳定主线，只通过 PR 合并。
- `develop`：可选的集成开发分支，若启用则用于集中尚未稳定的改动。
- `impl/<组件>-<实现名>`：实现开发分支。
- `feat/<组件>-<描述>`：功能开发分支。

## 基本要求

- 新实现或新功能优先从最新 `main` 拉出分支。
- 未经评审不要直接推送到 `main`。
- 若启用 `develop`，需要明确哪些任务先进 `develop`，哪些可直接进 `main`。
- 分支合并后应及时清理临时开发分支。

## 版本整理

- 阶段性版本概述维护在 `docs/exports/release-overview/`。
- 架构快照维护在 `docs/architecture/snapshot.md`。
- 阶段目标维护在 `docs/roadmap/todolist.md`。

## 推荐命名

- `impl/fs-devfs`
- `impl/driver-block-virtio`
- `feat/ipc-pipe-real`
- `feat/mm-user-space`
