# K-24 线上 Cargo manifest 兼容性修复（2026-08-05）

## 现象与根因

线上平台执行 `make -C os all` 时，在解析
`components/wateros-network/Cargo.toml` 的 `api-v0 = {` 处报告
`invalid inline table, expected }`。网络聚合 crate 和 smoltcp 实现 crate 共使用 8 个
跨行 inline dependency table；本地新 Cargo 接受这种 TOML 1.1 风格，但线上解析器按
旧 inline-table 单行规则拒绝，因此尚未进入 Rust 编译。后续 `Updating aliyun index`
只是依赖解析重试，不是网络组件或内核网络栈故障。

## 修改

按照仓库其它组件的 dependency 风格，把两个 manifest 中以独立 `{` 开始的跨行
inline table 改为 `name = { path = ..., package = ..., ... }` 形式；较长 feature 数组
沿用现有 manifest 的数组换行方式。依赖包名、路径、版本、optional、default-features
和 feature 列表均保持不变；没有修改 Rust 源码、网络架构或 task 模块。

## 验证

- Taplo 检查两个 manifest 通过，网络目录不再存在跨行 inline dependency table。
- 当前 Cargo 与 Cargo 1.86 均能完成 workspace metadata 解析。
- 修改前后 metadata 中两个网络 package 的依赖契约一致。
- `make all` 成功生成双架构 Final 候选。
- `make rv_check` 与 `make la_check` 通过。

Cargo 1.80 会在根 manifest 因不支持仓库既有的 Rust 2024 edition 而提前退出，因此不
属于可支持的线上工具链范围；本次错误明确发生在已开始解析 edition 2024 workspace
后的 TOML inline table。
