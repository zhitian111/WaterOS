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
- Linux power-up writes `CTL.RESET`, waits 10 ms, writes `CTL.EXTCLK`, clears
  interrupt bits `[9:0]` through `INT`, then writes the same `[9:0]` mask to
  `IEN`. It does not document RESET as self-clearing, so WaterOS does not poll
  or depend on self-clear behavior.
- `CSTS.ON=bit8` and `DSTS.RXON/TXON=bits0/1` are the documented command/data
  active indicators. WaterOS performs a bounded post-reset idle check before
  allowing clock configuration; this additional fail-closed check is not a
  claim that Linux uses the same preflight policy.
- Command completion, response timeout and response CRC status are `INT`
  bits 6, 7 and 8. Linux acknowledges observed status through the same W1C
  register. WaterOS additionally isolates ownership after every post-MMIO
  failure and requires idle plus a verified known-status clear before reuse;
  this recovery typestate is a WaterOS safety policy, not an upstream claim.
- Linux maps `MMC_RSP_PRESENT` to `CCTL.WAIT_RSP` and `MMC_RSP_136` to
  `CCTL.LONG_RSP`. Its threaded completion path reads RSP0 through RSP3 in
  increasing offset order for every command, then clears CARG and CCTL.
  WaterOS uses the same long-response register order and cleanup order, but
  minimizes reads to zero/one/four words for none/short/long descriptors.
  That smaller access policy and its physical response mapping require board
  validation.
- Linux MMC core separately defines response CRC, card-busy and opcode-check
  flags. The Loongson2 driver does not consume those flags: `CCTL.CHECK`,
  `INT.RESPCRC` and `INT.BUSYEND` are defined, but CHECK is not programmed and
  BUSYEND/RESPCRC do not drive its command completion state machine. WaterOS
  therefore rejects requested CRC/busy policies before MMIO instead of
  inferring behavior from register names. A spontaneously observed RESPCRC is
  still treated as a fail-closed anomaly.
- The second 2K1000 DT register region is an APB DMA configuration register.
- 2K1000 uses external APB DMA (`rx-tx`); it is not a DesignWare MMC host.
- Linux programs data requests in `DCTL -> BSIZE -> TIMER` order. DCTL carries
  a 12-bit block count plus START, external-DMA and bus-width bits; block size
  is also 12-bit and Linux rejects sizes not divisible by four. Its advertised
  maximum block count and block size are both 4095.
- For 2K1000 the external DMA slave source/destination is the main controller's
  `DATA` register at offset `0x40`, with 4-byte bus width. Read data uses
  device-to-memory direction. The driver submits and issues the DMA descriptor
  before sending CMD17/CMD18 through the normal command path.
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
10 ms reset timing sufficiency, CSTS/DSTS post-reset behavior, response word
ordering, command-error W1C/readback recovery, interrupt delivery, DMA routing
and cache coherency. Also validate that no-response/short-response commands
permit minimized RSP reads and that CARG/CCTL cleanup readback is reliable.
CRC checking, opcode checking and R1b busy completion require separate board
evidence before their descriptor policies can be enabled.
