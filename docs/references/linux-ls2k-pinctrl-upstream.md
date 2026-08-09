# Linux Loongson-2 pinctrl 参考

本页记录 2K1000 MMC pinctrl topology 与只读诊断所依据的 Linux 主线一手来源。

## 来源与许可证

- `arch/loongarch/boot/dts/loongson-2k1000-ref.dts`，SPDX `GPL-2.0`。
- `arch/loongarch/boot/dts/loongson-2k1000.dtsi`，SPDX `GPL-2.0`。
- `Documentation/devicetree/bindings/pinctrl/loongson,ls2k-pinctrl.yaml`，SPDX `(GPL-2.0-only OR BSD-2-Clause)`。
- `drivers/pinctrl/pinctrl-loongson2.c`，SPDX `GPL-2.0+`。
- `drivers/base/pinctrl.c`，Linux device-core pinctrl state 选择路径。

链接：

- <https://github.com/torvalds/linux/blob/master/arch/loongarch/boot/dts/loongson-2k1000-ref.dts>
- <https://github.com/torvalds/linux/blob/master/arch/loongarch/boot/dts/loongson-2k1000.dtsi>
- <https://github.com/torvalds/linux/blob/master/Documentation/devicetree/bindings/pinctrl/loongson%2Cls2k-pinctrl.yaml>
- <https://github.com/torvalds/linux/blob/master/drivers/pinctrl/pinctrl-loongson2.c>
- <https://github.com/torvalds/linux/blob/master/drivers/base/pinctrl.c>

## 已确认事实

- 参考板 MMC 的 `default` state 包含 `sdio -> sdio` 与 `pwm2 -> gpio` 两个映射；后者使 GPIO22 可作为 active-low card detect。
- pin controller 位于 `0x1fe00420`，主线 DTS 的 MMIO window 为 `0x18` 字节。
- 主线驱动将 SDIO group 映射到首个 mux 寄存器 bit20，将 PWM2 group 映射到 bit14。
- 驱动以 read-modify-write 设置 mux：非 GPIO function 置位，GPIO function 清位。因此期望 MMC 状态为 bit20=1、bit14=0。
- Linux device core 在设备 probe 前选择 `default` pinctrl state；不能把 DTS 中的 state 引用解释成固件已完成配置。

## WaterOS 边界

- topology 只接受参考板已证明的两个映射，缺少任一映射或额外未知映射均 fail-closed。
- bring-up plan 将受支持 state 标为 `RequiresDriver`，并保留独立 `PinControlUnavailable` blocker。
- 只读 snapshot 一次读取首个 mux 寄存器；它能报告瞬时选择状态，但不会解除 blocker，也不会执行修复写入。
- 真实 MMIO endian、pin ownership、firmware 并发修改和 bit 语义均为 `UNVERIFIED_ON_HARDWARE`。
