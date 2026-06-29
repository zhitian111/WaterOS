# wateros-driver — 新增 impl 指南

## 用途

说明如何为 `wateros-driver` 增加新的子系统实现或平台 bring-up impl：需改动的 `Cargo.toml`、feature 名称、必须实现的类型，以及接线位置。

事实来源：各层 `Cargo.toml`、`src/lib.rs`、现有 `impl-qemu-*` 与 `impl-dummy` 模式。

## 架构分层

```text
wateros-driver (聚合)
├── driver-api/api-v0          # 跨子系统模型（通常不需 per-platform impl）
├── driver-block               # 块子系统聚合
│   ├── block-api/api-v0       # BlockDevice trait + 注册表
│   └── block-impl/impl-*      # 具体块驱动
├── driver-character           # 字符子系统（同上）
├── driver-network             # 网络子系统（同上）
└── driver-impl/impl-*         # 平台：DTB/PCI 扫描 + 调用子系统注册
```

## 新增块设备实现（`block-impl/impl-<name>/`）

### 1. 创建 crate

- 路径：`driver-block/block-impl/impl-<name>/`
- 依赖：`wateros-driver-block-api-v0`（`api_v0`）、必要时 `wateros-driver-api-v0`、`frame_alloctor`、`virtio-drivers` 等。

### 2. 实现契约

```rust
impl BlockDevice for MyBlockDevice {
    fn read_blocks(&mut self, start: Lba, buf: &mut [u8]) -> DriverResult<()> { ... }
    fn write_blocks(&mut self, start: Lba, buf: &[u8]) -> DriverResult<()> { ... }
}
```

### 3. 注册到 `driver-block/Cargo.toml`

```toml
impl-my = { path = "./block-impl/impl-my/", package = "wateros-driver-block-impl-my", optional = true }

[features]
impl-my = ["dep:impl-my"]
```

### 4. 再导出（`driver-block/src/lib.rs`）

```rust
#[cfg(feature = "impl-my")]
#[doc(inline)]
pub use impl_my::MyBlockDevice;
```

### 5. DTB 绑定（可选）

在 `BLOCK_SUPPORTED_DEVICES` 增加 `SupportedDeviceEntry`，并在平台 impl 的扫描循环中实例化后调用 `register_block_device`。

### 6. 块缓存（可选）

平台注册前用 `BlockCacheManager::wrap(inner, BlockCacheManager::default_config())` 包装；须在根 `wateros` 与 `wateros-driver` 同时启用 `impl-block-cache`。

## 新增网络 / 字符实现

步骤与块设备类似，契约分别为 `NetworkDevice` 与 `CharacterDevice`，注册函数为 `register_network_device` / `register_character_device`。

- 网络 + 协议栈：新网卡须能被 `SmoltcpAdapter::new(shared_device)` 使用；无需改 smoltcp 除非介质非以太网。
- 字符设备：可实现 `SerialPort` 并用 `SerialPortCharacterDevice::new` 包装。

## 新增平台 impl（`driver-impl/impl-<board>/`）

### 1. 创建 crate 并实现引导契约

| 符号 | 职责 |
|------|------|
| `init_when_boot(dtb_pa: usize)` | 保存 DTB；可选早期 UART |
| `physical_ram_end_exclusive() -> usize` | 供 MM 恒等映射与帧分配 |
| `init_after_boot() -> DriverResult<()>` | 探测 + 注册 + devfs 同步 |
| `test()` | 只读自检（勿重复注册） |

### 2. 挂入聚合 `wateros-driver/Cargo.toml`

```toml
impl-my-board = { path = "./driver-impl/impl-my-board/", ... }

[features]
impl-my-board = [
    "block/impl-...",   # 该平台使用的块传输
    "network/impl-...",
]
```

### 3. 选择 `active_impl`（`wateros-driver/src/lib.rs`）

```rust
#[cfg(feature = "impl-my-board")]
pub use impl_my_board as active_impl;
```

**约束**：同一构建中仅应启用一个平台 impl feature（与 `impl-dummy` 互斥使用）。

### 4. 根 `os/Cargo.toml`

在对应 QEMU/板级 feature 中加入 `driver/impl-my-board` 及所需子 feature（如 `driver/impl-block-cache`）。

### 5. devfs 协作

- RISC-V 路径：通过 `fs::devfs::active_impl::refresh()` 与 `set_dt_unsupported_paths`。
- 从平台 impl **依赖 `wateros-fs` 聚合 crate**，不要直接依赖 `fs-devfs` 子目录。

## 占位 impl（`impl-dummy`）

各子系统与平台均保留 `impl-dummy`：仅保证 `cargo check` 与 feature 图完整，**不能**用于真实 bring-up。新增子系统时请同步添加对应 dummy crate 并在 `api-v0` feature 中链接。

## 检查清单

- [ ] 新 crate 已加入 `wateros-driver/Cargo.toml` `[workspace].members`（若适用）
- [ ] `api-v0` feature 向下传递
- [ ] 聚合 `lib.rs` `#[doc(inline)]` 再导出公开类型
- [ ] 平台 impl 的 `init_after_boot` 幂等
- [ ] 块设备注册路径考虑 `impl-block-cache`
- [ ] 更新 `docs/exports/features/wateros-driver.md` 与 `docs/guides/device-driver.md`

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 |
