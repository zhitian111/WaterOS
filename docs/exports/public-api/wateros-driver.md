# wateros-driver — 聚合层公共 API

## 用途

列出根 crate `wateros` 通过 `driver` 依赖最终使用的对外接口。impl 细节见各子 crate rustdoc 与 `docs/exports/features/wateros-driver.md`。

## 模块树（`wateros-driver/src/lib.rs`）

```text
driver::
  api::*              # wateros-driver-api-v0
  block::*            # wateros-driver-block
  character::*        # wateros-driver-character
  network::*          # wateros-driver-network
  active_impl         # feature 选中的 platform-impl crate
  uart                # [riscv/loongarch impl] 板级 UART 辅助
```

## 聚合层入口

| 项 | 说明 |
|----|------|
| `supported_device_entries()` | 合并 block/character/network 的 `supported_devices()` |
| `init_when_boot(dtb_pa)` | 保存 DTB 物理基址；委托 `active_impl` |
| `physical_ram_end_exclusive()` | 物理 RAM 上界；QEMU impl 解析 DTB，否则 `base-config` 回退 |
| `init_after_boot()` | 扫描/注册设备；失败仅 `log::warn` |
| `test()` | API + 各子系统 + 平台 impl 自检链 |

## `driver::api`（`wateros-driver-api-v0`）

| 类型 / 函数 | 说明 |
|-------------|------|
| `DeviceType` | `Block` / `Character` / `Network` / `Unknown` |
| `MmioRegion` | DTB `reg` 首段 `{ base, size }` |
| `IrqLine` | `{ irq, parent? }` |
| `SupportedDeviceEntry` | 子系统声明的 `compatible` 绑定表项 |
| `DeviceInfo` | DTB 节点扫描摘要 |
| `DriverError` / `DriverResult<T>` | 跨子系统错误 |
| `test()` | 构造样例 `DeviceInfo` 烟测 |

## `driver::block`

再导出 `wateros-driver-block-api-v0` 全部符号，并附加：

| 项 | 说明 |
|----|------|
| `BLOCK_SUPPORTED_DEVICES` / `supported_devices()` | DTB 绑定声明表 |
| `block_subsystem_claims_device` | 按 `compatible` + `DeviceType` 判断是否由块子系统认领 |
| `VirtioBlkDevice` | [impl-virtio-mmio] MMIO 块设备 |
| `VirtioPciBlkDevice` 等 | [impl-virtio-pci] PCI 块设备 |
| `CachingBlockDevice` / `BlockCacheManager` | [impl-block-cache] 写穿缓存 |

### 块 API 契约（`block-api-v0`）

| 项 | 说明 |
|----|------|
| `BLOCK_SIZE` | 512 |
| `Lba` | 逻辑块地址 |
| `BlockDevice` | `read_blocks` / `write_blocks`；默认 `read_bytes` / `read_prefix` |
| `SharedBlockDevice` | `Arc<Mutex<Box<dyn BlockDevice>>>` |
| `register_block_device` | 返回注册下标 |
| `block_device_count` / `first_block_device` / `block_device_at` | 全局表访问 |

## `driver::character`

| 项 | 说明 |
|----|------|
| `CharacterDevice` / `SerialPort` | 字符 I/O 与 MMIO UART 辅助 trait |
| `register_character_device` 等 | 全局字符设备表 |
| `register_builtin_character_devices` | RTC + null stub（feature 组合） |
| `is_uart_compatible` | DTB UART 节点识别 |
| `RtcCharacterDevice` / `NullCharacterDevice` | [impl-rtc-stub] / [impl-null-stub] |

## `driver::network`

| 项 | 说明 |
|----|------|
| `NetworkDevice` | `mac_address` / `send` / `receive` |
| `register_network_device` 等 | 全局网卡表 |
| `VirtioNetDevice` / `VirtioPciNetDevice` | virtio 网卡实现 |
| `SmoltcpAdapter` | [impl-smoltcp] 设备适配 |
| `stack::*` | [impl-smoltcp] 协议栈 init/poll/socket API |
| `SocketRef` / `TcpStreamHandle` 等 | [impl-smoltcp] VFS fd 桥 |

### `network::stack` 主要符号（`impl-smoltcp`）

| 项 | 说明 |
|----|------|
| `init(ip, gateway)` | 创建 `NetworkStack` |
| `poll` / `poll_at_millis` | 协议栈轮询 |
| `create_tcp_socket` / `create_udp_socket` | socket 工厂 |
| `socket_bind` / `listen` / `connect` / `accept` / `send` / `recv` | socket 操作 |
| `socket_setsockopt` / `socket_getsockopt` | iperf 依赖的子集 |
| `poll_socket_events` | Connecting → Connected 状态同步 |

## 平台 impl（`active_impl`）

| impl | 主要符号 |
|------|----------|
| `impl-qemu-riscv64-opensbi` | `init_when_boot`、`init_after_boot`、`physical_ram_end_exclusive`、`scan_device_info`、`virtio_blk_probe_test`、`uart` |
| `impl-qemu-loongarch64-virt` | 同上（PCI 路径）；`uart::with_default_uart` |
| `impl-dummy` | 占位 `add`（非 bring-up 路径） |

## 初始化契约（根 crate 责任）

1. `driver::init_when_boot(boot_dtb_pa)` — MM/平台初始化前或紧接其后。
2. `driver::active_impl::init_after_boot()` — 内核主线直接调用以获取 `DriverResult`（与聚合层 `driver::init_after_boot` 包装二选一）。
3. `driver::network::stack::init` — 网卡注册后、socket syscall 使用前（由 syscall/任务层触发）。
4. 周期性 `driver::network::stack::poll_at_millis` — 网络 poller 任务。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
