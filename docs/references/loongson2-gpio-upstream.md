# Loongson-2K GPIO upstream reference

WaterOS 的 LS2K1000 GPIO card-detect 只读模型依据 Linux 主线资料重新实现，没有复制或
vendor Linux C 源文件。

## 来源与许可证

- Linux `drivers/gpio/gpio-loongson-64bit.c`，SPDX `GPL-2.0+`：
  <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/gpio/gpio-loongson-64bit.c>
- DT binding `loongson,ls-gpio.yaml`，SPDX `GPL-2.0-only OR BSD-2-Clause`：
  <https://www.kernel.org/doc/Documentation/devicetree/bindings/gpio/loongson,ls-gpio.yaml>
- MMC binding/example，SPDX 以对应上游文件为准：
  <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Documentation/devicetree/bindings/mmc/loongson,ls2k0500-mmc.yaml>

## WaterOS 使用的已证明事实

- LS2K1000 使用 `loongson,ls2k1000-gpio` 和 `loongson,ls2k-gpio` fallback。
- bit-control bank 的 direction、output、input 偏移分别为 `0x00`、`0x10`、`0x20`。
- direction bit 为 1 表示 input；input bit 为 1 表示物理高电平。
- 参考 MMC 节点使用 GPIO22 和 `GPIO_ACTIVE_LOW` card detect。

## 保留边界

WaterOS 不改变 direction/output/mux/interrupt。64-bit volatile read、pinmux 初值、GPIO22
是否确实接到卡槽、上拉/去抖和插拔瞬态均为 `UNVERIFIED_ON_HARDWARE`。snapshot 结果不会
自动解除 MMC card-detect blocker。
