# QEMU RISC-V virt 机器驱动手册

[Driver 总览](../../README.md) · [公共 DTB 解析](../impl-common/README.md)

该 profile 从 platform 保存的 DTB 构建设备摘要，注册 UART/builtin 字符设备和所有可识别的 `virtio,mmio` block/net/可选 GPU/input。它不扫描 PCI，也不处理中断队列或热拔插。

```text
init_after_boot
  -> AtomicBool 防重复（失败会清除以允许重试）
  -> scan_device_info(DTB)
  -> probe_character_devices（DTB UART；无命中回退 UART0；注册 RTC/null）
  -> probe_virtio_devices（快照扫描表，按硬件 DeviceType 选择唯一子系统）
  -> 构造 transport/DMA 成功后追加全局设备注册表
  -> devfs::sync(unsupported paths)
```

`INIT_AFTER_BOOT_DONE.swap(true, AcqRel)` 令并发第二次调用直接 Ok；只有 `scan_device_info` 这类顶层 Err 会清回 false。单个 VirtIO 构造错误被转成 warning/unsupported，整体仍成功，因此不会自动 retry。成功返回也允许 block/network 数量为 0；根文件系统依赖块设备的启动层必须另行检查。

## DTB 摘要与硬件探测

`DEVICE_INFOS: Mutex<Vec<DeviceInfo>>` 每次 scan 先 clear，再遍历所有有 compatible 的节点。只保存 compatible 列表首项为主名，同时保留完整列表、第一段 MMIO、简化 IRQ、node name 与探测类型。

VirtIO 类型不是只信 DTB：对 compatible=virtio-mmio 且有 reg 的节点直接 volatile 读 `base+0` magic 和 `base+8` device id，magic 必须是 `0x74726976`，id 1/2/16/18 映射 net/block/display/input。读取前没有验证 `MmioRegion.size >= 12`；畸形的短 reg 仍会越界访问声明窗口。应先检查大小、页表映射与地址加法。

UART 由 compatible claim；有 MMIO 就每个都注册。若全局 character registry 此时数量仍为 0，才回退固定 UART0。这个条件观察的是全局所有字符设备，不只是本 profile UART：若其它代码已注册 `/dev/null`，可能抑制 UART fallback。更准确应检查本轮匹配 UART 数或 Serial kind。

之后无条件 `register_builtin_character_devices()`；若 init 允许重试或其它 profile 已注册，需确认 builtin 注册具有幂等性，否则可能重复节点。

Goldfish RTC 从摘要找 compatible，要求 region≥8，按 low 后 high 的锁存顺序读取 UTC ns；值 0 被当 IoError。摘要必须先 scan，RTC 读取不能早于此链。

## VirtIO claim、注册与诊断表

注册先 clone 整个 `DEVICE_INFOS` 快照，因此 transport negotiation/DMA 不持摘要锁；clone 和临时 Vec 使用内核 heap且不是 fallible。每个 VirtIO 节点依次用各子系统 claim，加硬件 device_type 双重确认；block/network 是互斥 `if/else if`，GPU/input 仅在尚未 handled 时尝试。

设备完整构造成功后才包装 Arc<Mutex<Box<dyn Trait>>>、进入全局 registry，并把 MMIO 记入 `VIRTIO_BLK/NET/GPU_MMIO`。input 当前没有对应 MMIO 诊断 Vec。失败、无 reg、未知 type 或 feature 被关闭的路径进入 `/dev/sys/<sanitized-node>` unsupported 列表；node name 的 `@`/`/` 替换为 `_`，不同名字可能碰撞，devfs 应处理重复。

若启用 block-cache，块设备先由 `BlockCacheManager::wrap` 包装；裸 block 与缓存路径的 flush/Drop 行为都要测试。设备 registry 只增不减，当前诊断 Vec clear 不会注销上轮实际设备。

VirtIO HAL 从物理帧分配器申请连续、清零页，并假定 vaddr==paddr；初始化必须在 MM/frame allocator 和内核 RAM 恒等映射之后。连续分配中途失败要回收全部已取页。

## 新增 MMIO 设备步骤

补 supported catalog/claim、id 分类、transport crate feature、完整构造、全局 registry、成功诊断表、devfs 节点和 QEMU 参数。失败必须进入 unsupported 诊断且不发布半对象。若设备可被两个子系统 claim，要规定唯一优先级，不能靠 if 顺序偶然决定。

## 回归清单

- 零/坏/短 DTB reg、坏 magic、未知/重复 node、路径 sanitize 碰撞；
- DTB 有多个 UART、无 UART、已有非 UART character 时仍正确 fallback；
- 每类 VirtIO 成功/feature关闭/构造失败，unsupported 与 registry 一致；
- snapshot/临时 Vec OOM 的启动行为；DMA OOM/非连续回滚；
- block0 已知读取、cache flush，MAC、空/小 RX、GPU/input 可选路径；
- init 并发/重复调用不重复注册；scan 失败后 retry；
- devfs 节点数、诊断 MMIO 表与实际 registry 对齐。
