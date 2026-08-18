# 22 SD 识别时钟 400kHz（修复真机 ResponseCrc）

## 任务内容

修复 SD host 激活失败：`sd init: Card(ResponseCrc)`。

根因：`activate_and_register_readonly` 用 `MMC_INPUT_FREQUENCY_HZ=100M` /
`target=100M` 直接把卡时钟定在 50MHz 上做识别。SD 规范要求识别阶段
（CMD0/CMD8/ACMD41/CSD）时钟 100–400kHz；50MHz 快了约 100 倍，卡响应
采样处于边缘，控制器报响应 CRC 错。U-Boot 的流程也是先 400kHz 识别、
识别完成后切 50MHz（SD High Speed）传输。

## 实施方案

1. `impl-dw-mmc/sd.rs`：新增 `SdCard::from_transport(transport, info)`，
   用于“重新配置控制器时钟后不丢卡状态地重新包装”。
2. `impl-jh7110-visionfive2/mmc.rs` `activate_and_register_readonly` 改为
   两段式：
   - 识别计划（`max_frequency_hz=400_000`）→ `initialize_controller` +
     `SdCard::initialize`，卡状态（RCA/CSD/addressing）在 400kHz 下建立；
   - `into_transport` → `configure_card_clock(100M, 50M)` 提到传输时钟 →
     `from_transport` 重新包装；
   - `register_readonly_block_device`（首扇区样例读在 50MHz 下进行）。
3. eMMC host（无卡）仍在卡初始化处超时，日志保留为预期噪音。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/src/sd.rs`
- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "configure_card_clock"
codegraph explore "SdCard::initialize"
codegraph explore "into_transport"
```

## 验收方式

- [ ] `cargo test -p wateros-driver-block-impl-dw-mmc` 通过。
- [ ] `cargo test -p wateros-driver-impl-jh7110-visionfive2` 通过。
- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] 真机 SD host 激活成功，`registered block device #...`，随后 fs 探测
      到 `/dev/vda4`。

## 验收命令

```bash
cd os/components/wateros-driver
cargo test -p wateros-driver-block-impl-dw-mmc
cargo test -p wateros-driver-impl-jh7110-visionfive2
cd ../../.. && make jh7110_check && make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
git diff --check
```

## 验证环境

- L0 宿主机：单测/check。✅
- L3 真机：400kHz 识别 + 50MHz 读首扇区。🔴（本次已复现 ResponseCrc）

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `impl-dw-mmc/sd.rs`：新增 `SdCard::from_transport(transport, info)`，
    用于改时钟后不丢卡状态地重新包装。
  - `impl-jh7110-visionfive2/mmc.rs` `activate_and_register_readonly`：
    两段式时钟——识别计划 `max_frequency_hz=400kHz` →
    `initialize_controller` + `SdCard::initialize`；随后
    `into_transport` → `configure_card_clock(100M, 50M)` →
    `from_transport` → 注册只读块设备。
- 验收结果：
  - `cargo test -p wateros-driver-block-impl-dw-mmc`、
    `cargo test -p wateros-driver-impl-jh7110-visionfive2`：通过。
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 SD host（`0x16020000`）在 400kHz 完成识别、50MHz 读首扇区并
    注册块设备；随后 `[fs]` 探测到 `/dev/vda4`；
  - 若仍有 `Card(ResponseCrc)`，下一步排查响应类型位/CLKSRC 配置；
  - 若识别通过但首扇区读失败，日志会给出 `register: Read(...)`，
    据此检查数据线/4-bit 切换。
