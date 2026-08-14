# 10 Loongson 2K1000 SATA/AHCI 块设备

## 任务内容

用宽松许可 crate 为 2K1000 接入 SATA/AHCI 块设备，替代旧分支里未验证的手写 AHCI：

- `isomorphic_drivers`（MIT/Apache）的 `block::ahci::AHCI`；
- `pci`（robigalia，MIT/Apache）做 PCI 配置空间扫描（基址 `0xfe00_0000`），找到
  class=0x01/subclass=0x06 的 SATA 控制器。

采用 **polled PIO** 读写（无中断），因此第一阶段不依赖任务 11 的外部中断。

## 实施方案

1. 引入并登记 `isomorphic_drivers` + `pci` 到第三方依赖清单/`*_SOURCE.md`。
2. 新增 `block-impl/impl-ahci`，实现 `BlockDevice`：512B 扇区与内核 `BLOCK_SZ` 的换算、
   DMA provider（用任务 02 的 VirtIO DMA 思路或 frame allocator 提供物理连续页）。
3. PCI 扫描在 2K1000 的 `0xfe00_0000` MMIO CAM 上枚举 AHCI，取其 BAR 基址初始化。
4. 补 host 单测：扇区换算、DMA 对齐、PCI class 匹配逻辑（用 fixture BAR 描述）。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-driver/driver-block/block-impl/impl-ahci/**`（新增）
- `os/components/wateros-driver/driver-block/src/lib.rs`、`Cargo.toml`
- `os/components/wateros-driver/driver-impl/impl-loongson2k1000la/**`

CodeGraph：

```bash
codegraph explore "BlockDevice"
codegraph explore "read_block"
codegraph explore "write_block"
codegraph explore "register_block_device"
```

## 验收方式

- [ ] host 单测通过（扇区换算/DMA 对齐/PCI 匹配）。
- [ ] `--features loongson2k1000la,pre` 能编译。
- [ ] 真机 SATA 至少能枚举 AHCI 并读一个扇区（真机项）。
- [ ] `isomorphic_drivers`/`pci` 已在第三方依赖与许可证清单登记。

## 验收命令

```bash
cd os
make configure
make la_check
cargo test -p wateros-driver-block-impl-ahci   # 以实际 package 名为准
git diff --check
```

## 验证环境

- L0 宿主机：单测 + `cargo check`。✅
- L2 板级 QEMU fork：2K1000 QEMU fork 的 SATA（若有）。🟠
- L3 真机：真实 SATA 控制器与盘读/写。🔴（必须）

## 任务简报

（完成后追加，格式见目录 README。）
