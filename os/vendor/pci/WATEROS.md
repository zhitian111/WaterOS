# WaterOS 本地 pci 副本

来源：robigalia/pci（MIT/Apache-2.0，见 `LICENSE-MIT`/`LICENSE-APACHE`）的
LoongArch 扩展 fork，取自参考仓库 NPUcore-IMPACT 的 `dependency/pci`（该目录自身
许可为 MIT/Apache-2.0，不受仓库整体 GPL 影响）。

扩展点（相对上游 <https://gitlab.com/robigalia/pci>）：

- `CSpaceAccessMethod::MemoryMapped`：MMIO CAM 配置空间访问；
- `scan_bus(ops, am, base_addr)`：带配置空间基址的总线扫描；
- `PortOps` 方法改为 `unsafe`。

用途：Loongson 2K1000 的 PCIe ECAM 配置空间探测（基址 `0xfe00_0000_00`）。
升级依赖时需重跑 `impl-ahci` 单测与 `loongson2k1000la` 构建检查。
