# 可派发实现任务（3 份，不同模块）

以下任务都只做 `impl`，不改 `api-v0` 签名。

---

## 任务 1（MM 模块）  
**标题**：完善 `impl-stack` 帧分配器安全性

**模块路径**：
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/`

**输入依赖**：
- `wateros-mm-frame-alloctor-api-v0::PhysicalFrameAllocator`

**要做内容**：
1. 增加重复释放检测（double free）并返回 `InvalidFrame`。
2. 增加非法范围保护（init 后只允许释放到该范围内）。
3. 增加统计接口（可选函数）：当前空闲帧数量、总帧数量。

**验收标准**：
- `test_with_range` 覆盖 double free 场景。
- `cargo check` 通过。
- 日志包含 `[frame-alloctor::impl-stack]` 前缀。

---

## 任务 2（MM 模块）  
**标题**：完善 `impl-sv39` 的页表回收与错误语义

**模块路径**：
- `os/components/wateros-mm/mm-impl/impl-sv39/`

**输入依赖**：
- `wateros-mm-api-v0::AddressSpaceOps`

**要做内容**：
1. 实现中间页表页的回收（当前只回收 root）。
2. `unmap` 后尝试回收空中间页，避免长期泄漏。
3. 细化错误返回：区分 `NotMapped` / `AlreadyMapped` / `Unsupported`。

**验收标准**：
- `test_with_range` 新增“多页 map/unmap 后空表回收”断言。
- map/protect/unmap/translate 行为不回归。
- `cargo check` 通过。

---

## 任务 3（Driver 模块）  
**标题**：实现设备注册器（DeviceManager）并向上统一可查询

**模块路径**：
- `os/components/wateros-driver/driver-impl/impl-qemu-riscv64-opensbi/`
- `os/components/wateros-driver/src/lib.rs`

**输入依赖**：
- `wateros-driver-api-v0::DeviceInfo`

**要做内容**：
1. 增加 `DeviceManager`（按 `DeviceType` 和 `compatible` 查询）。
2. `scan_device_info()` 结果写入管理器，不再靠“first()”探测。
3. 顶层 `wateros-driver` 导出查询接口（例如 `list_devices()`、`first_block_device()`）。

**验收标准**：
- 启动日志能打印设备总数与分类数（block/char/net）。
- virtio-blk 探测走注册器查询路径。
- `driver::test()` 覆盖“扫描 + 查询 + probe”。

---

## 统一协作规则（发给实现同学）

1. 仅改 `impl-*` 与顶层聚合导出，不改 API 签名。  
2. 所有新增行为都要有 `test()/test_with_*()`。  
3. 日志统一 `log::trace!/info!/warn!`，前缀带模块名。  
4. 若发现 API 不足，先提 issue，不直接改 API。
