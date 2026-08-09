# Loongson-2K clock upstream reference

WaterOS 的 2K1000 clock 只读模型依据 Linux 主线公开资料重新实现，没有复制或 vendor
Linux C 源文件。

## 来源与许可证

- Linux `drivers/clk/clk-loongson2.c`，SPDX `GPL-2.0+`：
  <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/clk/clk-loongson2.c>
- DT binding `loongson,ls2k-clk.yaml`，SPDX `GPL-2.0-only OR BSD-2-Clause`：
  <https://www.kernel.org/doc/Documentation/devicetree/bindings/clock/loongson,ls2k-clk.yaml>
- clock ID header，SPDX `GPL-2.0-only OR BSD-2-Clause`：
  <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/include/dt-bindings/clock/loongson,ls2k-clk.h>
- MMC binding/example，SPDX 以对应上游文件为准：
  <https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/Documentation/devicetree/bindings/mmc/loongson,ls2k0500-mmc.yaml>

## WaterOS 使用的已证明事实

- 2K1000 controller window 为 `0x58` bytes，输入为名为 `ref_100m` 的 reference clock。
- `LOONGSON2_APB_CLK` ID 为 12；2K1000 MMC binding example 使用这个 ID。
- APB 父链由 DC PLL、GMAC divider 和 APB scale 构成。
- DC PLL 位于 `0x20`：multiplier `[41:32]`、divisor `[31:26]`。
- GMAC divider 位于 `0x28` 的 `[27:22]`，Linux 使用 one-based、allow-zero divider。
- APB scale 位于 `0x50` 的 `[22:20]`，输出为 parent × (`field + 1`) / 8。

## 保留边界

寄存器读取宽度、固件初值、PLL lock/stability、并发 clock framework owner 和实际输出频率均为
`UNVERIFIED_ON_HARDWARE`。WaterOS 当前不写这些寄存器，也不因一次 snapshot 就解除 MMC
activation blocker。
