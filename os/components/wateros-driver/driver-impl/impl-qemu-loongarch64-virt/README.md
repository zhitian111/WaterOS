# QEMU LoongArch64 virt 机器驱动手册

[Driver 总览](../../README.md) · [LoongArch Platform](../../../wateros-platform/platform-impl/impl-qemu-loongarch64-virt/README.md)

该 profile 初始化默认 UART，并从 PCIe ECAM 枚举 bus 0 的第一个 VirtIO PCI block/net/GPU 和全部 input。它为裸 `-kernel` 环境重新分配 BAR、解析 vendor capability 并启用 MEMORY_SPACE/BUS_MASTER；不扫描 RISC-V virtio-mmio。

`INIT_AFTER_BOOT_DONE.swap(AcqRel)` 防并发重复。`register_devices` 捕获所有四类 PCI probe 错误并继续，最终通常返回 Ok，所以单个设备失败不会清 guard/retry；成功也允许没有块盘。根文件系统依赖块设备时，启动层必须检查 count，而非只看 init 返回。

```text
register_devices
  -> 清成功诊断 BDF 表
  -> find_config_base（DTB 或默认 ECAM）
  -> probe virtio-blk/net/gpu/input
  -> 配 BAR + 构造 PciTransport + DMA
  -> 完整成功后注册 Arc<Mutex<Box<dyn Device>>> 并记录 BDF
  -> 注册 builtin char + UART
  -> devfs sync
```

## ECAM、RTC 与枚举限制

`find_config_base` 只接受 DTB 中恰好等于 `0x2000_0000` 的 pci reg，否则静默回退该常量；它不会采用其它合法 ECAM base。内核页表必须覆盖配置窗口与整个 `0x4000_0000..0x8000_0000` BAR 区。后端只扫 bus 0，不穿 bridge。

LS7A RTC 从 DTB 找 `loongson,ls7a-rtc`，先保留其它 control 位并开启 TOY/output，再按 year-before/calendar/year-after 最多重试三次避免跨年撕裂。当前只显式校验 sec/min/hour/mday 上限，month/year 完整合法性依赖 `rtc_time_to_ns`。MMIO region 必须至少覆盖到 control+4。

## BAR 区间的真实现状

四个 probe 各自新建独立单调 allocator：block 从 `0x4000_0000`、net 从 `0x5000_0000`、GPU 从 `0x6000_0000`、input 从 `0x7000_0000` 开始，但它们的 end 全是 `0x8000_0000`。

这些并不是严格分区：block allocator 可以越过 0x50000000，net 可越过 0x60000000，随后与其它 allocator 分到相同地址。QEMU 常规小 BAR 通常没碰撞，但畸形/大 BAR 或未来多设备会重叠并导致两个 bus master 操作同一 MMIO 地址。应把 end 分别设为下一段起点，或由一个共享 allocator 统一管理全部 function；回归必须记录实际 `[addr,addr+size)` 并做全局 overlap 检查。

每类 probe 都从头创建 allocator，因此重新 probe 同一设备不会记得旧分配。PCI backend 初始化也不回滚已经写的 BAR、command 和 cursor。当前 registry 无注销；诊断 BDF Vec clear 只清记录，不会移除已注册设备或禁用 bus master。

## 注册顺序与失败语义

顺序是 block → net → optional GPU → optional all input → builtin character → UART。每个成功对象先包装/可选 block-cache，再注册，最后向对应 BDF Vec push。probe Err 只 warning；input 的 all-probe 若后一个 function 失败，会整批 Err、此前局部设备 Drop，但已写 PCI 配置不回滚，且本轮一个 input 都不注册。

`uart::init_default_virt_uart` 初始化一个平台全局串口对象，随后 `register_uart_character_device` 又创建/注册字符包装；需保持 early console、默认 UART global 和 character device 对同一寄存器的并发访问策略，避免两个独立锁交错字节。

块设备存在时立即做 block0 read 自检，失败只 warning且设备仍保留。自检不能写磁盘；更深的写/flush测试必须使用专用镜像。

## 新增 PCI 设备步骤

先给所有子系统设计唯一共享 BAR 规划，再实现 transitional/modern ID、只读 candidate scan、事务配置/rollback、transport 构造、registry/devfs 与 BDF 诊断。多 function 顺序按稳定 BDF；单项失败是跳过还是整批失败要显式决定。

## 回归清单

- DTB ECAM 正确/其它合法地址/缺失，默认 base 可访问性；
- 空 bus、非 bus0、未知 ID、畸形/循环 capability；
- 32/64/I/O/Below1MiB/大 BAR，所有已分配 range 全局不重叠；
- 每个失败点后的原 BAR、command、bus master、DMA/frame 与 registry；
- block/cache 读和 flush、net MAC/小 RX、GPU framebuffer、多个 input；
- input 中间坏 function 的整批失败现状；
- early/runtime/character UART 并发输出不交错；RTC 跨年和字段错误；
- init 重复/并发不重复注册，设备缺失仍启动但强依赖层正确阻止继续。
