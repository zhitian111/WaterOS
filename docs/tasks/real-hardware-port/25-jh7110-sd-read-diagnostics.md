# 25 SD 单块读失败寄存器诊断日志

## 任务内容

为 `read_single_block` 增加失败时寄存器级诊断，定位 SD host 注册阶段
`Read(IoError)` 的**具体底层错误**（数据超时 / 数据 CRC / FIFO / 响应错误）。

背景：`register_readonly_block_device` 把底层 `MmcError` 映射为
`DriverError::IoError`，真机日志只剩 `register: Read(IoError)`，无法区分
数据通路哪一位出错。

## 实施方案

1. `impl-dw-mmc` 增加 `log` 依赖。
2. `mmc.rs` 新增 `DwMmc::read_failure(err)`：读取并打印
   `RINTSTS`/`STATUS`/`CTRL`/`RESP0`/`fifo_offset`；
   `read_single_block` 的错误返回路径（`check_errors`、FIFO 分支、
   poll 超时）统一经它打印后再返回原始错误。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/Cargo.toml`
- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "read_single_block"
codegraph explore "check_errors"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机失败日志包含 `[dw-mmc] read_single_block failed err=... rintsts=...`
      ，据此确定下一步修复方向。

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
- L3 真机：抓取寄存器级错误。🔴

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc` 增加 `log` 依赖；
  - `mmc.rs` 新增 `read_failure`：失败时打印 `RINTSTS`/`STATUS`/`CTRL`/
    `RESP0`/`fifo_offset`，`read_single_block` 三条错误路径统一接入。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`：12 passed。
  - `cargo test -p wateros-driver-impl-jh7110-visionfive2`：18 passed。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 抓取 `[dw-mmc] read_single_block failed err=... rintsts=...` 一行，据此
    定位是 DRTO/DCRC/FRUN/SBE/EBE 中的哪一个，再做对应修复。
