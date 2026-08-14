# 07 DW MMC 块设备接入（SD 只读）

## 任务内容

从 `feat/real-hardware-common`/`feat/visionfive2-port` 迁移共享 `impl-dw-mmc`
（`block-impl/impl-dw-mmc`，含 `mmc.rs`/`sd.rs`），接入 WaterOS block API，先支持
JH7110 的 SD 只读路径。

优先评估现成轮子 `dwmmc-host`（+ `sdmmc-protocol`）能否替代/收窄手写状态机；不满足
则保留迁移来的实现并补单测。JH7110 的 tuning/DLL 路径如轮子不支持，先 fail-closed。

## 实施方案

1. 迁移 `impl-dw-mmc` 的 `mmc.rs`/`sd.rs`，对齐 `driver-block` 的 `BlockDevice` 契约。
2. 在 `impl-jh7110-visionfive2` 驱动里接 `mmc.rs`（bus-width/时钟/reset 门控）。
3. 评估并决定是否引入 `dwmmc-host`/`sdmmc-protocol`；若引入，记录 license 与适配点。
4. 补命令序列/状态机 host 单测（无真硬件时以 fixture 驱动状态跳转）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-dw-mmc/**`（新增）
- `os/components/wateros-driver/driver-block/src/lib.rs`
- `os/components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/mmc.rs`

CodeGraph：

```bash
codegraph explore "BlockDevice"
codegraph explore "read_block"
codegraph explore "register_block_device"
```

## 验收方式

- [ ] DW MMC 命令/状态机 host 单测通过。
- [ ] block 后端能通过 facade 编译并注册。
- [ ] SD 只读在真机至少能枚举/读一个扇区（真机项，可延后到 08 一并验收）。

## 验收命令

```bash
cd os
make configure
make rv_check
cargo test -p wateros-driver-block-impl-dw-mmc   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：状态机单测。✅
- L2 板级 QEMU fork：JH7110 MMC 仿真（若有）。🟠
- L3 真机：真实 SD 枚举/读。🔴

## 任务简报

（完成后追加，格式见目录 README。）
