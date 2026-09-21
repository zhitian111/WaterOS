# WaterOS 本地 isomorphic_drivers 副本

来源：<https://github.com/rcore-os/isomorphic_drivers>（master，克隆于 2026-08-15）。
该 crate 未发布到 crates.io，故保留本地副本。

用途：`block::ahci::AHCI`（AHCI/SATA polled PIO 主机控制器驱动）。

许可证：上游仓库未附带 LICENSE 文件；rCore 生态惯例为 MIT/Apache-2.0，本副本按
MIT 登记（见 [`LICENSE-MIT`](LICENSE-MIT)）。**引入前请在项目许可证清单中复核。**

本副本未做 WaterOS 定制改动。升级依赖时需同步更新版本与摘要，并重跑
`impl-ahci` 的 host 单测与 `loongson2k1000la` 构建检查。
