# Driver Block API 实现说明

对应 API：
- `os/components/wateros-driver/driver-block/block-api/api-v0/src/lib.rs`

## 目标

定义块设备最小读写契约，为后续 VFS/ext4/ramfs 提供统一底座。

## 已定义接口

- 常量：`BLOCK_SIZE = 512`
- trait：`BlockDevice`
  - `read_blocks(start_block, buf) -> DriverResult<()>`
  - `write_blocks(start_block, buf) -> DriverResult<()>`

## 实现方必须完成

1. 读写语义正确
   - `start_block` 按块号解释。
   - `buf.len()` 必须满足实现要求（通常是 `BLOCK_SIZE` 的整数倍）。
2. 错误处理
   - 越界/IO 失败返回 `DriverError::IoError` 或兼容错误。
3. 与具体驱动绑定
   - `virtio-blk` 实现应将底层库错误映射为 `DriverResult`。

## 日志与自检要求

- 使用 `log` 宏。
- 推荐 test 覆盖：
  - 读 block0 成功（smoke test）
  - 写后回读一致（若当前阶段允许写）
  - 不合法参数时返回错误而不是 panic
