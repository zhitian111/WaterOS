# WaterOS 本地 VirtIO 驱动副本

此目录来自 crates.io 的 `virtio-drivers 0.12.0`，上游 Git 提交为
`12685bf34793b4a07795c2add01c76a2575815cc`，原始 crate SHA-256 为：

```text
a7a6012590bf8cc11f57abb1b5a8470e2366353bf352386b48650a87b5538204
```

保留本地源码是为了给 `VirtIOGpu` 增加公开的矩形刷新入口。WaterOS 的改动只涉及：

- 校验矩形非空、坐标加法不溢出且不越过 scanout；
- 按线性 BGRA8888 framebuffer 计算 backing offset；
- 对同一矩形依次发送 `TRANSFER_TO_HOST_2D` 和 `RESOURCE_FLUSH`；
- 增加 offset、零尺寸、越界和溢出单元测试。

上游许可证见 [`LICENSE`](LICENSE)。升级依赖时必须重新核对 VirtIO GPU 命令语义、更新
版本与摘要，并同时运行本目录测试以及 WaterOS 双架构 `user-graphics` 构建检查。
