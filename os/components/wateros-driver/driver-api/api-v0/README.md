# Driver API v0 离线开发手册

[Driver 总览](../../README.md) · [机器公共解析](../../driver-impl/impl-common/README.md)

本 crate 定义跨设备类别的**发现数据模型**、粗粒度错误和机器驱动入口，不定义块、网卡、
字符、显示或输入 I/O。各设备的运行期 trait 位于对应 `driver-*/...-api/api-v0`。

## 1. 分层和调用链

```text
platform 保存 DTB PA / PCI 根信息
  -> 当前 machine impl 扫描总线
     -> 构造 DeviceInfo
     -> 与各子系统 SupportedDeviceEntry 匹配
     -> 读取 transport device id / PCI capability
     -> 选择唯一 block/net/display/input/char 构造器
     -> 子系统 registry 发布运行期设备

os bring-up
  -> driver::init_after_boot()
  -> driver::machine().init_after_boot()
  -> profile-specific enumerate + register
```

`SupportedDeviceEntry` 只表示“可以尝试绑定”，不证明设备存在或初始化成功。VirtIO-MMIO
的多个子系统都会声明同一个 `virtio,mmio` compatible，必须再读 transport device id，
不能让所有构造器同时绑定。

## 2. 数据结构

### 2.1 `DeviceType`

```text
Block / Character / Network / Display / Input / Unknown
```

它是扫描阶段的摘要，不等于驱动对象的 Rust 类型。`Unknown` 应保留在诊断快照但不发布
设备。enum 当前没有 `#[non_exhaustive]`；增加 variant 会使所有穷尽 match 编译失败，
必须同步扫描、注册、devfs 和测试。

### 2.2 `MmioRegion`

`base` 是物理地址，`size` 是字节数。数据结构本身不强制：

- `size != 0`；
- `base + size` 不溢出；
- 页对齐或落在平台已映射 MMIO窗；
- 与 RAM/其他设备不重叠。

这些都必须在 probe/map 前验证。不要把物理 base 直接当用户/内核可解引用地址，除非
当前平台明确提供恒等映射或 DMW。

### 2.3 `IrqLine`

```rust
pub struct IrqLine { pub irq: u32, pub parent: Option<u32> }
```

只表达一个简单线号和可选 phandle，无法完整表示多 cell specifier、trigger/polarity、
`interrupts-extended`、MSI/MSI-X、PLIC context 或 GPIO级联。复杂描述要在 platform/
transport 专属类型中解析；不能把第一个 cell 当成完整 IRQ后声称成功。

### 2.4 `SupportedDeviceEntry`

三个 `'static str`：subsystem、诊断 name、精确 compatible。它适合静态数组，无 heap。
匹配应遍历 `DeviceInfo.compatibles` 完整列表，而不是只看 legacy `compatible` 字段。

### 2.5 `DeviceInfo`

```text
node_name       DTB node name，可能含 @unit-address
compatible      完整列表的首项兼容字段（legacy/日志）
compatibles     所有 NUL 分隔字符串
device_type     transport probe 后分类
mmio            当前只保留首个 reg region
irq             当前只保留简单 irq
```

结构没有强制 `compatible == compatibles[0]`，构造器必须维护此不变量。它拥有 `String/Vec`，
所以扫描发生在 heap 初始化后；early boot 不能使用它做零分配探测。

## 3. 错误模型

| `DriverError` | 使用场景 |
| --- | --- |
| `InvalidDtb` | FDT header/layout 无效 |
| `InvalidParam` | 对齐、长度、溢出、flag 契约错误 |
| `NotFound` | DTB PA/设备/资源不存在 |
| `Unsupported` | transport、feature、复杂描述不支持 |
| `IoError` | 寄存器、queue、设备操作失败的折叠结果 |

当前没有 `OutOfMemory/Busy/AlreadyInitialized/Timeout`。不要把所有错误都无差别转
`IoError` 后依赖日志猜测；需要上层恢复策略时应扩展错误或在子系统 API保留更细分类，
再在机器边界单向折叠。

`DriverResult<T>` 只是别名，不带 errno。向 VFS/network 转换时由消费者明确映射，禁止
把 enum discriminant 直接作为用户 errno。

## 4. `MachineDriver`

```rust
pub trait MachineDriver {
    fn init_after_boot(&self) -> DriverResult<()>;
    fn realtime_ns(&self) -> DriverResult<Option<u64>> { Ok(None) }
    fn test(&self);
}
```

- `init_after_boot`：枚举+注册。trait 本身没有规定幂等；当前 QEMU实现用 init guard。
- `realtime_ns`：`Ok(None)` 表示该机器无 RTC能力，不是时间 0；Err 表示探测/I/O失败。
- `test`：运行机器级只读/受控自检；不能假设所有可选设备存在。

初始化发布原则：先在局部对象中完成 feature negotiation、queue/DMA分配和硬件 ready，
最后一次性插入 registry。失败时撤销 IRQ handler、DMA和 queue，不能留下 registry 可见
的半设备。若允许重试，init guard 必须从 InProgress/Failed 回到可重试状态。

## 5. 新增设备类别实例

以新增 RNG 为例：

1. 在 API中增加 `DeviceType::Entropy`（或先复用明确的子系统机制）；
2. 新建 entropy API trait，定义同步、阻塞、错误和熵质量，而非放进本 crate；
3. 根 driver 聚合 `entropy::supported_devices()`；
4. DTB VirtIO device id/PCI device id 分类为 Entropy；
5. machine register 只调用 entropy transport 构造器；
6. 成功后发布 registry/devfs `/dev/hwrng`；
7. 更新 dummy、RV、LA profile 的穷尽 match和测试；
8. 明确无设备时 boot 是继续、降级还是失败。

新增 `compatible` 时只改静态 claim 不够；若 transport probe 没有识别 device id，它仍会
保持 Unknown。

## 6. 锁和生命周期

扫描用 `DeviceInfo` 快照应与运行期设备 registry 分离。不要持 DEVICE_INFOS 全局锁去做
MMIO、DMA分配、日志或 registry callback；先 clone/移动局部快照，再逐设备初始化。

运行期设备通常由 `Arc<dyn Trait>` 注册，registry拥有强引用；IRQ回调、VFS handle和
network stack可能再持引用。热拔插前必须先阻止新获取、停 IRQ/queue、等待使用者退出，
最后 drop DMA。当前 QEMU路径主要假设设备不热拔插。

## 7. 自检与限制

`api_v0::test()` 只分配一个样例 `DeviceInfo` 并检查字段，不访问 DTB/硬件，也不覆盖
匹配/错误/生命周期。它需要 heap 和 logger。

回归矩阵：

- 多 compatible、首项一致性、Unknown、无 reg/irq；
- base+size 溢出、零 size、MMIO与 RAM重叠；
- 同 compatible 多子系统声明后由 device id唯一绑定；
- probe 成功但 queue失败时 registry不可见；
- duplicate init、部分失败重试、零设备；
- RTC Some/None/Err；
- RV MMIO与 LA PCI/MMIO两条机器路径。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
