# WaterOS 本地 dwmac-rs 副本

来源：<https://github.com/elliott10/dwmac-rs>，提交
`e918894a3f973f81f14fbe518b21f23994e6e9df`（2026-02-12）。上游该提交的
`Cargo.toml` 声明 MIT 许可证，但仓库树中未附 LICENSE 文件；本副本补充标准 MIT
许可证文本，见 [`LICENSE-MIT`](LICENSE-MIT)。

用途：VisionFive 2 / JH7110 的 DWMAC 5.20 GMAC 与 YT8531C PHY 驱动。

本副本当前保持上游源码内容，WaterOS 的 MMIO、DMA 和网络设备适配位于
`components/wateros-driver/driver-impl/impl-jh7110-visionfive2/src/gmac.rs`。
升级时必须同步记录提交、复核许可证，并重跑 `make jh7110_check` 与真机网络验证。
