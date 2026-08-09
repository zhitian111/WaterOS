# VisionFive 2 MMC source and license audit

Audit date: 2026-08-10

## Sources consulted

| Source | License stated by source | Facts used |
|---|---|---|
| Linux `starfive,jh7110-mmc.yaml` binding | GPL-2.0-only OR BSD-2-Clause | compatible, two clocks, one IRQ, optional sysreg, FIFO properties |
| Linux `jh7110.dtsi`, `jh7110-common.dtsi`, `jh7110-starfive-visionfive-2.dtsi` | GPL-2.0 OR MIT | host addresses, IRQ 74/75, bus widths, 100 MHz maximum, eMMC/SD board roles |
| Linux `dw_mmc.h` and `dw_mmc.c` | GPL-2.0-or-later | public DesignWare register offsets and bit definitions used as hardware-interface facts |
| Linux `dw_mmc-starfive.c` | GPL-2.0-only | JH7110 has a StarFive-specific sample phase and DDR clock behavior |

Primary locations:

- <https://www.kernel.org/doc/Documentation/devicetree/bindings/mmc/starfive%2Cjh7110-mmc.yaml>
- <https://github.com/torvalds/linux/blob/master/arch/riscv/boot/dts/starfive/jh7110.dtsi>
- <https://github.com/torvalds/linux/blob/master/arch/riscv/boot/dts/starfive/jh7110-common.dtsi>
- <https://github.com/torvalds/linux/blob/master/arch/riscv/boot/dts/starfive/jh7110-starfive-visionfive-2.dtsi>
- <https://github.com/torvalds/linux/blob/master/drivers/mmc/host/dw_mmc-starfive.c>
- <https://github.com/torvalds/linux/blob/master/drivers/mmc/host/dw_mmc.h>

## Reuse decision

No Linux driver implementation was copied or translated. The WaterOS module is an
independent implementation of the documented MMIO protocol, with repository-style
Rust APIs and original tests. Register numbers and bit positions are necessary
hardware interface facts. The GPL-only StarFive implementation was used only to
identify work that must remain deferred: sample-phase tuning, DDR clock doubling,
clock/reset control and board-specific syscon programming.

The current code is therefore only a controller primitive plus DTB discovery. It
must not be represented as a functioning SD/eMMC driver until card enumeration,
clock/reset/pinmux, voltage switching, tuning and cache/DMA behavior are completed
and exercised on physical hardware.
