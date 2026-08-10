# WaterOS 驱动复用与许可证清单

本清单区分“复用了一个通用软件组件”和“已经适配目标板”。许可证字段只记录仓库/锁定依赖中可核对的来源；`UNVERIFIED_ON_HARDWARE` 表示尚未在 VisionFive 2 或 Loongson 2K1000LA 实机验证。

| 组件/适配层 | 当前用途 | 来源与许可证证据 | 可复用范围 | 状态与下一步 |
| --- | --- | --- | --- | --- |
| `virtio-drivers` `=0.12.0` | virtio-blk MMIO/PCI、virtio-input、virtio-net 的队列与传输 | [Cargo.toml](../../os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/Cargo.toml)；宿主锁定 crate `virtio-drivers-0.12.0` 声明 MIT；上游 [rcore-os/virtio-drivers](https://github.com/rcore-os/virtio-drivers) | QEMU virt RISC-V/LoongArch 的 virtio 设备；不能代替 2K1000LA GMAC/USB 控制器驱动 | QEMU/host 路径可编译；目标板 `UNVERIFIED_ON_HARDWARE`。下一步是板上设备树与 DMA/cache 验证。 |
| `smoltcp` `=0.12.0` | TCP/UDP/IP 协议栈和远程 debug monitor 的 socket 层 | [Cargo.toml](../../os/components/wateros-network/network-impl/impl-smoltcp/Cargo.toml)；宿主锁定 crate `smoltcp-0.12.0` 声明 0BSD；上游 [smoltcp-rs/smoltcp](https://github.com/smoltcp-rs/smoltcp) | 协议层可跨网卡复用；不包含目标板 MAC/PHY 驱动 | QEMU virtio-net smoke/host 测试；真实网卡、PHY、IRQ `UNVERIFIED_ON_HARDWARE`。 |
| WaterOS `impl-uart-16550` | NS16550 兼容字符设备、轮询读写和 `/dev/ttyS*` 注册 | [实现](../../os/components/wateros-driver/driver-character/character-impl/impl-uart-16550/src/lib.rs)；WaterOS 自有代码，无外部依赖许可证可继承 | 仅限寄存器布局和时钟已确认的 16550 兼容 UART；不是 Loongson UART 事实证明 | host 单测/target 编译通过；2K1000LA MMIO、IRQ 和 pinmux `UNVERIFIED_ON_HARDWARE`。 |
| WaterOS `impl-dw-mmc` | DesignWare MSHC 的 PIO/轮询和 SD 命令状态机 | [实现](../../os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs)；WaterOS 自有代码，无外部依赖许可证可继承 | 可作为控制器协议层候选；必须重新绑定目标板 clock/reset/pinctrl/card-detect/DMA | mock register/card 测试通过；2K1000LA SD/eMMC 电气和寄存器语义 `UNVERIFIED_ON_HARDWARE`。 |
| WaterOS 2K1000LA topology/IRQ/MMC/DMA | DTB 发现、能力快照、IRQ owner、MMIO 诊断和 DMA typestate | [LA driver](../../os/components/wateros-driver/driver-impl/impl-loongson2k1000la/src/lib.rs)；WaterOS 自有代码，无第三方许可证混入 | 仅为 2K1000LA 目标板的适配骨架；不能移植到 QEMU virt | host/target 编译及纯软件状态机测试；真实寄存器副作用、cache、rearm `UNVERIFIED_ON_HARDWARE`。 |
| `ext4plus` vendor dependency | 物理根盘 ext4 镜像和 VFS 后端 | [vendor README](../../os/vendor/ext4plus/README.md) 与 LICENSE；MIT OR Apache-2.0 | 文件系统后端，不是硬件驱动；镜像工具固定 `64bit`/无 journal 契约 | QEMU/host 镜像验证通过；掉电恢复和真实 SD/eMMC 写屏障 `UNVERIFIED_ON_HARDWARE`。 |

## 复用准入规则

1. 第三方组件必须锁定版本并在本表记录许可证证据；不能因为 API 相似就把 QEMU 驱动标记为真实板驱动。
2. 目标板驱动在获得 DTB、时钟/reset、IRQ、DMA/cache 和数据通路证据前，必须保留 `UNVERIFIED_ON_HARDWARE`。
3. 新增第三方代码时，把上游链接、许可证文件位置、修改范围和发布义务先写入本表，再进入平台分支。
4. MIT/0BSD/Apache-2.0 兼容性不等于 GPL/LGPL 合规；若未来引入 GPL 驱动，必须单独评估链接和发布边界。
