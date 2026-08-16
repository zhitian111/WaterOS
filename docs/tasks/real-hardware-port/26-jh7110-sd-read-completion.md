# 26 SD 读完成判定：DTO 即完成（修真机 err=Fifo 误报）

## 任务内容

修复 SD 读首扇区的完成判定误报 `err=Fifo`。

真机寄存器证据：`err=Fifo` / `rintsts=0x8`（DTO） / `status=0x8906`
（FIFO 空） / `resp0=0x900`（R1 状态 TRAN + READY_FOR_DATA）。即 CMD17
成功、卡处于传输态、512 字节已读满，但循环把“字节已满 + FIFO 残留”误判
为 FIFO 错误，且成功条件额外要求 `command_done`（数据命令的完成信号是
DTO，而非 command_done；U-Boot 数据阶段同样只判 DTO）。

## 实施方案

1. `read_single_block` 成功条件改为 `data_over && bytes == output.len()`，
   去掉 `command_done` 要求与相关局部变量。
2. 删除“`bytes == len && fifo_words > 0 → Fifo`”误判分支；真实 FIFO 错误
   仍由 `check_errors` 的 FRUN/SBE/EBE 位捕获。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "read_single_block"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 SD host 注册成功，`registered block device #...`，fs 探测到
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
- L3 真机：读首扇区闭环。🔴（本次已定位到完成判定误报）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/mmc.rs` `read_single_block`：成功条件改为
    `data_over && bytes == 512`（去掉 `command_done` 要求与局部变量）；
    删除“`bytes == 512 && fifo_words > 0 → Fifo`”误判分支；`check_errors`
    接入 `read_failure` 诊断。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 SD host `registered block device #...`，fs 探测到 `/dev/vda4`，
    进入下一阶段（挂载 rootfs / 写路径）。
