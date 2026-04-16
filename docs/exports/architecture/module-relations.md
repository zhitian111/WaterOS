# 模块关系说明

## 统一模式

WaterOS 主要使用以下模块关系：

- 根 crate 聚合一级组件。
- 一级组件聚合子组件。
- `api-v0` 提供契约。
- `impl-*` 提供具体实现。
- 聚合 `src/lib.rs` 负责最终导出。

## 典型示例

### platform

- `platform-api/api-v0`：平台引导、时间等契约。
- `platform-arch`：架构相关能力。
- `platform-firmware`：固件能力。
- `platform-impl/impl-qemu-riscv64-opensbi`：当前默认平台实现。
- `platform/src/lib.rs`：根据 feature 导出统一接口。

### driver

- `driver-api/api-v0`：总驱动能力契约。
- `driver-block`、`driver-character`、`driver-network`：子系统聚合层。
- `driver-impl/impl-qemu-riscv64-opensbi`：当前主线实现。
- `driver/src/lib.rs`：统一导出并提供初始化入口。

### mm

- `mm-api/api-v0`：MM 契约。
- `mm-frame-alloctor`：帧分配子组件。
- `mm-impl/impl-sv39`：当前默认实现。
- `mm/src/lib.rs`：聚合导出并组织自检。
