# Loongson 2K1000 SATA/AHCI 真机闭环报告

日期：2026-08-16

状态：**AHCI/SATA 块设备已在 Loongson 2K1000LA 真机闭环**

对应历史任务：[10 Loongson 2K1000 SATA/AHCI 块设备](../10-loongson2k1000-sata-ahci.md)

## 1. 结论

WaterOS 已在 Loongson 2K1000LA 的片上 SATA 控制器上识别 Kingchuxing 32GB SSD，注册
块设备，读取 MBR 分区表，并将第一分区上的 ext4 文件系统以读写模式挂载。启动过程中随后
对 `/etc` 下多个文件的覆盖和读取校验成功，证明本次结果不只停留在 IDENTIFY 或单扇区读取，
块读写与 ext4 RW 路径都已实际工作。

已验证的链路为：

```text
PCI 00:08.0 / BAR0 0x400e0000
  -> AHCI HBA 初始化
  -> ATA IDENTIFY DEVICE
  -> 62533296 个 512-byte 扇区
  -> WaterOS block device #0
  -> MBR 第一分区 /dev/vda1
  -> ext4 RW mount
  -> 文件覆盖与回读校验
```

启动末尾的 `/glibc/busybox: NotFound` 是当前根文件系统缺少对应用户程序或路径配置导致，
不属于 AHCI、分区解析或 ext4 挂载失败。整盘 `/dev/vda` 的 ext4 magic 校验失败也符合预期：
ext4 位于第一分区 `/dev/vda1`，而不是整盘起始扇区。

## 2. 验证环境

| 项目 | 实际值 |
|---|---|
| 板卡 / SoC | Loongson 2K1000LA |
| 启动方式 | U-Boot 通过 TFTP 加载 WaterOS legacy uImage |
| SATA 控制器 | PCI `00:08.0`, device ID `0x7a08:0x0014` |
| AHCI BAR | `0x400e0000`, 长度 `0x10000` |
| 磁盘 | Kingchuxing 32GB, firmware `W0106A0` |
| 协商链路 | `DET=3`, `SPD=2`（3 Gbps）, `IPM=1` |
| 驱动实现 | `simple-ahci = 0.1.1-preview.1` + WaterOS HAL |
| 文件系统 | MBR 第一分区上的 ext4，RW 挂载 |

U-Boot 先前报告同一磁盘容量为 `62533296 x 512`，与 WaterOS IDENTIFY 结果一致。

## 3. 最终实现

最终方案复用了 StarryOS 在 2K1000 真机上验证过的
[`simple-ahci`](https://github.com/Starry-OS/simple-ahci)，精确固定版本
`0.1.1-preview.1`。WaterOS 的适配层负责：

1. 扫描 2K1000 PCI 配置空间并取得 SATA BAR0。
2. 将 AHCI MMIO 物理地址 `0x400e0000` 映射为 DMW0 直映 VA
   `0x80000000400e0000`。
3. 实现 `simple_ahci::Hal`：DMW0/DMW1 VA 到 PA 的转换、毫秒时钟以及 LoongArch
   `dbar 0` 发布屏障。
4. 将 `AhciDriver` 适配到 WaterOS `BlockDevice`，暴露容量并转发同步读写。
5. IDENTIFY 返回零容量时拒绝注册，避免底层命令失败被误判为设备可用。

关键源码：

- [`impl-ahci/src/lib.rs`](../../../../os/components/wateros-driver/driver-block/block-impl/impl-ahci/src/lib.rs)
- [`impl-ahci/Cargo.toml`](../../../../os/components/wateros-driver/driver-block/block-impl/impl-ahci/Cargo.toml)

旧的 `isomorphic_drivers` bring-up 和寄存器诊断代码目前仍保留作对照，但实际成功路径不再
调用它的 AHCI 构造函数。此次也不再调用 `prepare_soc_sata_phy()`，因此不会主动 pulse
SoC PHY/lane reset；固件留下的 IODMA window 和 `DMACR=0x66` 均保持不变。

## 4. 真机证据

### 4.1 IDENTIFY 与容量

```text
AHCI device: Kingchuxing 32GB ... W0106A0
[ahci] simple-ahci ready blocks=62533296 block_size=512
[driver][2k1000] AHCI/SATA registered as block device #0
```

这证明 IDENTIFY 已完成，容量不为零，并且设备已进入 WaterOS block facade。

### 4.2 命令完成状态

```text
after-init-success ... ghc=0x80000002 ... pi=0x00000001
after-init-success port0 ...
    tfd=0x00000050(sts=0x50 err=0x00)
    ssts=0x00000123(det=3 spd=2 ipm=1)
    ci=0x00000000
    dmacr=0x00000066
```

与旧实现中 `CI=1`、`TFD=0x1d0` 的超时状态相比，新路径的命令槽已清空，设备不再 BSY，
task-file error 为 0，链路保持 active。

### 4.3 分区读取与 ext4 RW

```text
[fs] init: probed root partition /dev/vda1
[fs::rootfs] mount root RW from /dev/vda
[fs::another-ext4] mount failed: ... "Invalid magic number"
[fs::rootfs] whole-disk mount failed; trying partition /dev/vda1
[fs::rootfs] mount root RW from /dev/vda1
[bringup][stage-00-bus] ext4 root mounted (RW)
[root-layout] overwrote /etc/passwd (122 bytes)
[root-layout] verified /etc/passwd contains nobody (122 bytes)
```

这一段同时验证了磁盘扇区读取、MBR 分区发现、ext4 元数据读取、文件写入和回读。

## 5. 与旧失败路径的差异

旧调试路径已经证明 MMIO、PHY 链路和 doorbell 可写，但在 IDENTIFY 或无数据命令中长期
停留于 `PxCI=1`。最终可工作的 `simple-ahci` 路径采用更接近 StarryOS 成功移植的最小接管
顺序：

- 不主动复位 SoC SATA PHY/lane；
- 不改写固件 IODMA window、SCTL 或 DWC 私有 DMACR；
- HBA reset 后直接接管已经由固件训练好的链路；
- 使用 `simple-ahci` 的 command list/FIS 初始化和轮询命令路径；
- descriptor 发布前由 HAL 提供 LoongArch barrier。

这组变化已经形成可工作的工程方案，但本次 A/B 没有把每一项差异单独隔离，因此不能把
根因收窄为某一个寄存器或某一次 reset。可以确定的是，高低 DMA 地址、PRDT 格式和磁盘硬件
本身都不是继续阻塞当前移植的理由。

## 6. 验收状态

- [x] PCI SATA 控制器和 BAR0 探测成功。
- [x] AHCI HBA 初始化成功。
- [x] ATA IDENTIFY 成功，型号和容量与 U-Boot 一致。
- [x] WaterOS 块设备注册成功并暴露容量。
- [x] MBR 第一分区识别成功。
- [x] `/dev/vda1` ext4 以 RW 模式挂载。
- [x] ext4 文件覆盖和回读成功。
- [ ] 长时间、多扇区和跨页 I/O 压力测试。
- [ ] 掉电/重启后的写入持久性与宿主机 `e2fsck -fn` 检查。
- [ ] AHCI 中断模式；当前按轮询路径使用。

本报告依据用户提供的 2026-08-16 真机完整启动日志。此次文档整理没有重新构建内核或重复
执行真机测试。

## 7. 后续工作与风险

1. `simple-ahci` 当前会设置 `GHC.IE` 和 `PxIE`，但 WaterOS 尚无 AHCI IRQ handler。
   后续应在轮询模式关闭这些中断使能，或完整接入中断处理。
2. `BlockDevice::flush()` 当前只返回成功，没有发送 ATA `FLUSH CACHE` (`E7/EA`)；ext4
   持久性验收前需要补齐真实 flush 语义。
3. 需要检查 `simple-ahci` 对跨物理页和非连续 I/O buffer 的假设；必要时使用连续 bounce
   buffer 或生成分段 PRDT。
4. 旧 `FrameProvider`、固定低地址 DMA、PHY reset helper 和 vendored AHCI 诊断代码已退出
   运行路径，确认无需回归后应清理或放入明确的诊断 feature。
5. 根 README 已同步 `simple-ahci` 及其新增传递依赖的许可证；发布前仍应按 Cargo 包中
   的许可证原文复核，`simple-ahci` 声明为 `MulanPSL-2.0 OR MIT`。
6. 下一阶段处理 rootfs 中 `/glibc/busybox` 缺失或路径选择，不应继续修改 AHCI 来掩盖该错误。

## 8. 任务简报

- 完成日期：2026-08-16
- commit：随本次 2K1000 真机移植闭环提交，见该分支对应的 `git log`
- 实际改动：以 `simple-ahci 0.1.1-preview.1` 替换实际 AHCI 执行后端，增加 WaterOS HAL、
  容量暴露、读写适配和零容量失败保护；保留原诊断实现作为暂时对照。
- 验收结果：2K1000LA 真机完成 IDENTIFY、块设备注册、MBR 分区扫描、`/dev/vda1`
  ext4 RW 挂载与文件写入/回读。
- 未验证/风险：压力 I/O、真实 flush、跨页 DMA、中断关闭/接入和断电持久性仍待验收；
  BusyBox `NotFound` 属于 rootfs 后续任务。
