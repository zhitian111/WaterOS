## 变更类型

- [ ] API 设计（仅核心开发，涉及 *-api/api-v0）
- [ ] 新实现（在 *-impl/impl-xxx 下实现既有 API）
- [ ] Bug 修复
- [ ] 重构
- [ ] 文档 / 脚本 / 配置
- [ ] 其他

## 涉及组件

<!-- 如：wateros-fs, wateros-ipc-pipe, wateros-driver-block -->

## 简要说明

<!-- 做了什么、为什么做 -->

## 与 API 的关系

<!-- 若是「新实现」：写明实现的 trait / API 版本（如 api-v0）；若修改了 API，请说明并标注需核心开发重点 Review -->

## 测试与验证

<!-- 如何验证：qemu 跑测、单元测试、或具体手动步骤 -->

## 检查清单

- [ ] 基于最新 `main`（或 `develop`）拉取并已 rebase/merge
- [ ] `cargo fmt` / `cargo clippy` 通过，无新增 warning
- [ ] 已在本地或 CI 中完成自测
- [ ] 若为实现类 PR：未修改 API 定义，或已与核心开发确认 API 变更

## 关联任务

<!-- 如有 Issue/看板任务，请填链接或 ID，例如：Closes #12 -->
