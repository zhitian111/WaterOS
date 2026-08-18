# 24 SD 数据通路修复：FIFO 偏移固定 0x200 + 强制 PIO 模式

## 任务内容

修复 SD host 注册失败：`register: Read(IoError)`（1-bit/25MHz 仍复现）。

对照同板 U-Boot（starfive-tech/U-Boot `JH7110_VisionFive2_devel`
`drivers/mmc/dw_mmc.c` + `include/dwmmc.h`）定位两处差异：

1. **FIFO 数据寄存器偏移**：U-Boot 固定 `DWMCI_DATA = 0x200`；我们的驱动
   按 `VERID >= 0x240A` 猜测 0x200/0x100。若猜测落到 0x100，读数据时一直
   从错误偏移取字，FIFO（0x200）里的数据无人消费 → 溢出
   （FRUN）→ `Read(IoError)`，与真机症状吻合。
2. **DMA/IDMAC 位**：U-Boot 用 IDMAC 传输后在 CTRL 清 `DMA_EN`（bit 5），
   `IDMAC_EN`（bit 25）可能遗留；我们走 PIO（FIFO 轮询）却从未显式关闭
   这两个位，若遗留会使数据被路由到 IDMAC 而非 FIFO。

## 实施方案

1. `impl-dw-mmc/mmc.rs` `probe()`：`fifo_offset` 固定 `0x200`（以本板 U-Boot
   为准），删除 VERID 猜测；`VERID` 常量随之删除。
2. `initialize_polling_with_bus_width`：`reset()` 后读 CTRL，清
   `DMA_ENABLE (1<<5)` 与 `IDMAC_ENABLE (1<<25)`，强制 PIO。
3. 更新受影响的 host 单测断言。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "probe"
codegraph explore "read_single_block"
codegraph explore "initialize_polling_with_bus_width"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过（含更新后断言）。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 SD host 注册成功（`registered block device #...`），fs 探测到
      `/dev/vda4`。

## 验收命令

```bash
cd os/components/wateros-driver
cargo test -p wateros-driver-block-impl-dw-mmc
cargo test -p wateros-driver-impl-jh7110-visionfive2
cd /home/zhitian/project/WaterOS_real_hardware_porting/os
make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：单测/check。✅
- L3 真机：FIFO/PIO 修复后首扇区读。🔴（本次仍复现 Read(IoError)）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/mmc.rs` `probe()`：`fifo_offset` 固定 `0x200`（同板 U-Boot
    `DWMCI_DATA`），删除 VERID 猜测与 `VERID` 常量；
  - `initialize_polling_with_bus_width`：`reset()` 后读 CTRL 清除
    `CTRL_DMA_ENABLE (1<<5)` / `CTRL_IDMAC_ENABLE (1<<25)`，强制 PIO；
  - 测试更新：mock 不再写 VERID，旧 fifo 布局测试改为断言固定 `0x200`。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 SD host `registered block device #...`，fs 探测到 `/dev/vda4`；
  - 若仍失败，下一步加 `read_single_block` 失败时 RINTSTS/STATUS/CTRL
    寄存器级诊断日志，定位具体错误位。
