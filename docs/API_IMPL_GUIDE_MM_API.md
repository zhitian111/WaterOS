# MM API 实现说明

对应 API：
- `os/components/wateros-mm/mm-api/api-v0/src/`

## 目标

定义地址空间管理的统一语义，屏蔽具体页表实现（Sv39/其他架构），为 syscall、loader、driver、task 提供稳定依赖。

## 关键接口

1. `AddressSpaceOps`（核心）
   - `satp_value`
   - `map_page_to_ppn`
   - `unmap_page_to_ppn`
   - `protect_page`
   - `translate_addr`
2. `UserMemoryOps`
   - `copy_from_user`
   - `copy_to_user`
3. `HeapBrk`
   - `brk_region`
   - `brk`
4. `MmapOps`
   - `mmap`
   - `munmap`

## 实现方必须完成

1. 映射一致性
   - `map_page_to_ppn` 对重复映射返回 `AlreadyMapped`。
   - `unmap_page_to_ppn` 对未映射返回 `Ok(None)` 或符合约定错误。
2. 权限更新
   - `protect_page` 不改变物理映射，只改变权限。
3. 地址翻译
   - `translate_addr` 能正确处理页内偏移。
   - 未映射地址返回 `Ok(None)`，不应 panic。
4. brk/mmap 最小语义
   - 先支持最小子集，未支持项返回 `Unsupported`。

## 日志与自检要求

- 使用 `log` 宏记录阶段信息（trace/info）。
- 推荐 test 结构：
  - `mm-api::test()` 调用 `addr/perm/flags` 子模块 test
  - `impl` 层 test 负责 map/protect/unmap/translate 行为断言

## 与 frame allocator 的关系

- `AddressSpaceOps` 默认 helper 假设分配器 `FrameId = PhysPageNum`。
- 若实现选择其他 FrameId，需在实现层补充转换逻辑，或调整实现接口适配层。
