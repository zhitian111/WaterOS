# Driver API 实现说明

对应 API：
- `os/components/wateros-driver/driver-api/api-v0/src/lib.rs`

## 目标

提供设备发现与驱动装配的统一数据模型，避免实现层直接依赖具体 DTB 节点细节。

## 已定义对象

- `DeviceType`：`Block/Character/Network/Unknown`
- `MmioRegion`：`base/size`
- `IrqLine`：`irq/parent`
- `DeviceInfo`：
  - `node_name`
  - `compatible`
  - `device_type`
  - `mmio`
  - `irq`
- `DriverError`、`DriverResult<T>`

## 实现方必须完成

1. 设备扫描
   - 从平台启动参数获得 DTB 基址（或等效信息）。
   - 解析设备节点，构建 `DeviceInfo` 列表。
2. 设备分类
   - 根据 `compatible + mmio header` 判定 `device_type`。
   - 未识别设备标注为 `Unknown`，不要直接 panic。
3. 注册与查询
   - 至少提供一个全局设备列表或注册器供上层查询。
4. 错误语义
   - DTB 无效、设备缺失、未支持能力要返回明确 `DriverError`。

## 日志与自检要求

- 使用 `log` 宏输出扫描阶段信息。
- 推荐 test 覆盖：
  - 扫描到的设备数量
  - 关键设备（如 virtio,mmio）是否被识别
  - 错误路径（无 DTB/无匹配设备）
