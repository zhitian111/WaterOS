# Loongson-2K MMC upstream reference

WaterOS's Loongson MMC register definitions and sequencing were independently
implemented with the following upstream Linux sources as behavioral references:

- Linux `drivers/mmc/host/loongson2-mmc.c`:
  <https://codebrowser.dev/linux/linux/drivers/mmc/host/loongson2-mmc.c.html>
- Linux DT binding change containing `loongson,ls2k1000-mmc` and the two register
  region descriptions:
  <https://android.googlesource.com/kernel/common/+/854ff7923753009189a9e1f80d23ae9d407c2fb2%5E1..854ff7923753009189a9e1f80d23ae9d407c2fb2/>

The referenced Linux driver is `SPDX-License-Identifier: GPL-2.0-only` and
copyright 2018–2025 Loongson Technology Corporation Limited. No Linux source
file is vendored into WaterOS. Any future direct code import must retain its
SPDX identifier and copyright notice and undergo project-level license review.

Confirmed facts used by the current WaterOS foundation:

- Main registers occupy offsets `0x00..0x64`; DATA is `0x40`, IEN is `0x64`.
- Command argument/control are `0x08/0x0c`; response words are `0x14..0x20`.
- The second 2K1000 DT register region is an APB DMA configuration register.
- 2K1000 uses external APB DMA (`rx-tx`); it is not a DesignWare MMC host.

Still requiring physical-board validation: MMIO accessibility/endian behavior,
clock/reset/power ordering, response word ordering, interrupt delivery, DMA
routing and cache coherency.
