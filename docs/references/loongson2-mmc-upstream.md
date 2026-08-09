# Loongson-2K MMC upstream reference

WaterOS's Loongson MMC register definitions and sequencing were independently
implemented with the following upstream Linux sources as behavioral references:

- Linux `drivers/mmc/host/loongson2-mmc.c`:
  <https://codebrowser.dev/linux/linux/drivers/mmc/host/loongson2-mmc.c.html>
- Linux DT binding change containing `loongson,ls2k1000-mmc` and the two register
  region descriptions:
  <https://android.googlesource.com/kernel/common/+/854ff7923753009189a9e1f80d23ae9d407c2fb2%5E1..854ff7923753009189a9e1f80d23ae9d407c2fb2/>
- Linux `drivers/dma/loongson2-apb-dma.c`:
  <https://codebrowser.dev/linux/linux/drivers/dma/loongson2-apb-dma.c.html>

The referenced Linux driver is `SPDX-License-Identifier: GPL-2.0-only` and
copyright 2018–2025 Loongson Technology Corporation Limited. No Linux source
file is vendored into WaterOS. Any future direct code import must retain its
SPDX identifier and copyright notice and undergo project-level license review.
The APBDMA reference is `GPL-2.0-or-later`, copyright 2017–2023 Loongson
Corporation; it is likewise referenced but not vendored.

Confirmed facts used by the current WaterOS foundation:

- Main registers occupy offsets `0x00..0x64`; DATA is `0x40`, IEN is `0x64`.
- Command argument/control are `0x08/0x0c`; response words are `0x14..0x20`.
- Controller-private clock registers are `CTL=0x00` and `PRE=0x04`;
  `CTL.ENCLK=bit0`, `PRE.EN=bit31`, while the prescaler field is documented as
  bits `[9:0]`. The current Linux path nevertheless clamps its computed
  prescaler to 255, writes `PRE.EN | divider`, then updates only `CTL.ENCLK`.
- Linux computes the divider with upward rounding. WaterOS mirrors that policy
  but requires coherent parent evidence, fresh controller reads and readback;
  its current conservative plan retains the upstream 255 clamp.
- The second 2K1000 DT register region is an APB DMA configuration register.
- 2K1000 uses external APB DMA (`rx-tx`); it is not a DesignWare MMC host.
- The APBDMA order register is accessed as a non-atomic 64-bit little-endian
  value: Linux uses `lo_hi_readq`/`lo_hi_writeq`, which access the low 32-bit
  word before the high 32-bit word. Transfer start writes zero first, then the
  descriptor address combined with `64BIT_EN | START`.
- Linux terminate, pause and final-IRQ paths encode `64BIT_EN | STOP` after
  preserving the descriptor-address bits. They do not poll an order bit or
  descriptor status to prove that hardware has become idle.
- The hardware descriptor contains a `stats` word, but the referenced driver
  defines no status bit meanings and does not inspect that word in its ISR.

Still requiring physical-board validation: MMIO accessibility/endian behavior,
`PRE`/`CTL.ENCLK` write and readback behavior, clock/reset/power ordering,
response word ordering, interrupt delivery, DMA routing and cache coherency.
