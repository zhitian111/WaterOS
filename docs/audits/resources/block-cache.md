# 块设备缓存槽（CachingBlockDevice 64 槽 LRU）— 资源生命周期审计

> 审计日期：2026-06-25  
> 资源编号：#19（`docs/audits/resource-inventory.md`）  
> 扫描范围：`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/**` 及跨模块调用链  
> Baseline：单核多线程；对照 Linux 块层读缓存语义（写穿、固定容量、LRU 淘汰）

## 资源概要

| 项 | 内容 |
|----|------|
| 资源名称 | 块设备 LBA 缓存槽（`CachingBlockDevice::slots`） |
| 所属组件 | `wateros-driver` / `driver-block` / `impl-block-cache` |
| 主要类型 | `CachingBlockDevice`、`Slot`、`BlockCacheConfig`、`BlockCacheManager` |
| 硬上限 | **64 块**（`wateros_base_config::fs::BLOCK_CACHE_CAPACITY_BLOCKS`）；每块 **512 B**（`BLOCK_SIZE`）→ 单设备约 **32 KiB** 槽缓冲 |
| 对外句柄 | `SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>`（包装后注册到全局表） |
| 策略 | 写穿（write-through）；`flush()` / `BlockCacheManager::flush_all()` 当前为 no-op |

## 1. 分配入口

### 1.1 槽位与缓冲预分配

| 函数 | 文件 | 触发条件 | 行为 |
|------|------|----------|------|
| `CachingBlockDevice::new` | `impl-block-cache/src/lib.rs` | `BlockCacheManager::wrap` 调用 | 从 `inner.block_size()` 取块大小；`capacity = config.capacity_blocks`（`block_size==0` 时强制为 0）；为每个槽 `vec![0u8; block_size]` 预分配堆缓冲 |
| `BlockCacheConfig::default` | 同上 | 未显式传配置时 | 读 `BLOCK_CACHE_CAPACITY_BLOCKS`（64） |
| `BlockCacheManager::wrap` | `impl-block-cache/src/manager.rs` | 平台 probe 成功初始化 virtio-blk 后 | `Box::new(CachingBlockDevice::new(inner, config))` → `Arc::new(Mutex::new(...))` |

**前置依赖**：

- 内核堆已 `init`（槽缓冲经 `alloc::vec` 分配；失败走 `#[alloc_error_handler]` → **panic**）。
- 底层 `VirtioBlkDevice`（或其它 `BlockDevice`）已成功构造。
- RISC-V QEMU 主线：`qemu-riscv64-opensbi` 默认启用 `driver/impl-block-cache` feature，probe 路径以缓存包装后注册。

**运行时「分配」**（无新堆槽，仅索引复用）：

| 函数 | 行为 |
|------|------|
| `alloc_slot` | `free.pop()` 取空闲下标；若无空闲则 `evict_lru_slot()` 回收 LRU 槽 |
| `cache_put` | 命中则原地更新；未命中则 `alloc_slot` + 写入 `map` / `lru` |
| `read_blocks`（未命中路径） | 合并连续未命中区间，一次 `inner.read_blocks`，再对每个块 `cache_put` |

**不分配槽位的路径**：

- `capacity_blocks == 0`：`new` 不建 `slots`/`free`；`read_blocks`/`write_blocks` 直接透传 `inner`。
- `write_blocks` 对**未缓存** LBA：仅写底层，不调用 `cache_put`（故意不冷启动缓存）。

### 1.2 平台注册入口（唯一生产路径）

```
init_after_boot (impl-qemu-riscv64-opensbi)
  → VirtioBlkDevice::from_mmio
  → [cfg block-cache] BlockCacheManager::wrap(dev, default_config())
  → register_block_device(shared)   // api-v0/src/lib.rs
```

LoongArch64 平台（`impl-qemu-loongarch64-virt`）虽在 `Cargo.toml` 声明 `block-cache` feature，但 **probe 代码未调用 `BlockCacheManager::wrap`**，直接注册裸 `VirtioBlkDevice`（见 §6）。

## 2. 回收入口

| 场景 | 是否存在 | 实现 |
|------|----------|------|
| LRU 淘汰（槽复用） | ✅ | `evict_lru_slot`：`lru.pop_front` → `slots[idx].lba.take()` → `map.remove`；槽索引交还 `cache_put` 复用 |
| 显式 `flush` / 卸载 | ⚠️ 无实质回收 | 写穿策略下无脏页；`flush` 为 no-op |
| 块设备 `unregister` | ❌ | `BLOCK_DEVICES` 仅 `push`，无 `unregister_block_device`（资源 #36 共性） |
| `CachingBlockDevice` / `Arc` Drop | ⚠️ 理论路径 | 最后 `Arc` 释放时 `Slot::data` `Vec` 与 `inner` `Box` 随 Rust Drop 释放；**正常运行时不会发生**（全局表 + devfs + 根 FS 长期持有 `Arc` clone） |
| 任务退出 / `close(fd)` | N/A | 缓存绑定全局块设备，不 per-task |
| 错误路径回滚 | ✅（槽层面） | `inner.read_blocks` 失败时不执行后续 `cache_put`，槽状态不变 |

**结论**：槽位在对象生命周期内**固定为 capacity 个**，靠 LRU 在固定池内周转；**无动态增长泄漏**；**无显式内核级 teardown**（与块设备注册表一致，故意常驻）。

## 3. 生命周期状态机

### 3.1 单槽状态

```
[未使用]  lba=None，下标在 free 栈
    │ cache_put / alloc_slot（从 free 弹出）
    ▼
[已占用]  lba=Some(LBA)，在 map 中，在 lru 队列
    │ touch_lru（读命中 / 写更新）
    ▼
[已占用·最近使用]  仍在 map/lru，lru 位置移至队尾
    │ evict_lru_slot（free 空且需新槽）
    ▼
[未使用]  lba=None，map 已删，等待 cache_put 重新绑定
```

### 3.2 整体对象状态

```
构造中 (CachingBlockDevice::new)
  → slots[0..capacity] 预分配，全部入 free
运行中
  → 不变量：|map| == |lru| == capacity - |free|
  → 持锁访问：SharedBlockDevice::lock() 独占 mut
内核常驻 (Arc 永不为 0)
  → 无「已释放」阶段
```

### 3.3 半初始化风险

- `new` 在循环中逐槽 `vec![0u8; block_size]`；若中途 OOM **panic**，无部分构造对象泄漏（Rust 构造失败丢弃未完成 `Self`）。
- `wrap` 仅包装已成功 `new` 的对象，无 partial register 回滚需求（注册发生在 `wrap` 返回之后）。

## 4. 账本稳定性

| 维度 | 结论 | 说明 |
|------|------|------|
| 分配/释放成对 | **稳定** | 固定 `capacity` 槽；淘汰只改 `lba`/`map`/`lru`，不增删 `slots` 向量 |
| 引用计数 / 所有权 | **稳定** | `inner` 由 `CachingBlockDevice` 独占；对外仅 `Arc<Mutex<...>>` 共享句柄 |
| double-free | **无** | 无手动 dealloc；槽索引不复用为双重 map 键 |
| use-after-free | **无** | 槽数据始终在 `slots` 存活期内有效 |
| 泄漏 | **无运行时泄漏** | 64 槽预分配后不再扩容；对象常驻等于故意持有 |
| 野指针 / 索引错乱 | **低**（依赖不变量） | `map: Lba → slot_idx` 与 `lru` 由同一 `&mut self` 路径维护；**无并发无锁访问** |
| 错误路径 partial alloc | **稳定** | 读失败不 `cache_put`；写先 `inner.write_blocks`，失败则不更新缓存 |

**综合结论**：**稳定**（固定池 + 写穿 + 单 Mutex 序列化）。主要风险为 **不变量破坏时 panic**（见问题列表），非账本漂移型泄漏。

## 5. 耗尽处理

| 场景 | 当前行为 | 与预期差距 |
|------|----------|------------|
| 槽位满 | `alloc_slot` → `evict_lru_slot` 静默淘汰最久未用块 | 符合固定容量 LRU 预期；写穿下淘汰无需写回 |
| `capacity == 0` | 透传底层，零槽内存 | 符合 `BLOCK_CACHE_CAPACITY_BLOCKS=0` 语义 |
| 堆 OOM（`new` 时） | `alloc_error_handler` → **panic** | 启动期一次性 ~32KiB/设备；应 warn+失败返回更合适（P2） |
| 非法参数 | `read_blocks`/`write_blocks` 返回 `DriverError::InvalidParam` | 合理 |
| 底层 I/O 失败 | 向上传播 `DriverError` | 合理 |
| LRU 不变量破坏 | `evict_lru_slot` 内 **`expect` panic** | 不应在生产路径触发；若触发则整机崩溃（P1） |

**不应拒绝却继续执行的路径**：未发现静默截断读写字节数或无限重试。

## 6. 跨资源耦合

```
VirtioBlkDevice (inner)
  ↑ owned by
CachingBlockDevice (64 slots)
  ↑ Box<dyn BlockDevice> in
Arc<Mutex<...>>  ←── BLOCK_DEVICES[#]（全局表，永久持有）
                 ←── DevFsImpl.block_bindings（clone Arc）
                 ←── SharedFs / ext4 mount（mount 时 clone device）
```

| 耦合对象 | 关系 | 生命周期注意 |
|----------|------|--------------|
| 内核堆 (#6) | `new` 时 64×`Vec<u8>` | 启动期固定占用；与页缓存 16MiB 独立 |
| 块设备注册表 (#36) | `register_block_device` 后永不移除 | 缓存随 `Arc` 常驻 |
| 页缓存 (#17) | VFS 文件页层，下层经 FS 调 `read_blocks` | 分层缓存；块缓存对 FS 透明 |
| `EXT4_SMALL_READ_CACHE` (#21) | ext4 在块设备之上再缓存 1 块 | 写路径 `invalidate_small_read_cache`；**不**失效 `CachingBlockDevice` 槽（写穿下由 `write_blocks` 更新已缓存行） |
| 锁 (#22–#23) | `SharedBlockDevice` 与 `BLOCK_DEVICES` 各一层 `spin::Mutex` | 持 `dev.lock()` 期间独占缓存+底层 I/O；与锁审计 `driver-block-char` 组交叉 |
| LoongArch 平台 | feature 存在但未接线 | 行为与 RISC-V 不一致（P2） |

**锁顺序提示**：ext4 `read_with_small_cache` 可能先锁 `EXT4_SMALL_READ_CACHE` 再锁 `dev`；`block_write_bytes` 持 `dev` 至返回前 `invalidate_small_read_cache`。单核下竞态窗口有限，但与页缓存/小块缓存叠加时存在**跨层陈旧读**理论风险（属 FS 层一致性，非块缓存槽账本问题）。

## 7. 潜在问题列表

### P0（泄漏 / UAF / 卡死 / 静默耗尽）

**本轮未发现确认的 P0 问题。**

固定容量 LRU 在写穿语义下账本闭合；无动态泄漏、无 UAF、无阻塞式耗尽重试。`expect` panic 属可靠性风险，归入 P1。

### P1（错误路径 / panic / 一致性）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| BC-P1-01 | 卡死/崩溃 | `evict_lru_slot` 在 `lru` 空或槽 `lba` 为 `None` 时 `expect` **panic**，整机不可恢复 | `lib.rs:98–100` |
| BC-P1-02 | 错误路径 | `alloc_slot` 依赖 `free`/`lru`/`map` 不变量，无防御性校验或 warn | `lib.rs:93–95` |
| BC-P1-03 | 一致性 | `write_blocks` 不填充冷块缓存；依赖写穿读盘，与「写后读走缓存」场景下多一次底层读（性能，非正确性） | `lib.rs:191–197` |

### P2（限额 / 错误码 / 平台差异）

| ID | 类型 | 描述 | 位置 |
|----|------|------|------|
| BC-P2-01 | 静默耗尽 | `new` 时堆分配失败直接 panic，无 warn 用量日志 | `lib.rs:61–65` + 全局 `alloc_error_handler` |
| BC-P2-02 | 平台差异 | LoongArch `block-cache` feature 未在 probe 使用 `BlockCacheManager::wrap` | `impl-qemu-loongarch64-virt/src/lib.rs:114–117` |
| BC-P2-03 | 可观测性 | 无 `used_slots/capacity` 统计或淘汰 warn，排查热集超出 64 块时困难 | 全模块 |
| BC-P2-04 | API 缺口 | `flush`/`flush_all` 为占位；将来若改 write-back 需补脏写回与失败回滚 | `lib.rs:81–84`, `manager.rs:31–33` |
| BC-P2-05 | 跨层缓存 | 与 `EXT4_SMALL_READ_CACHE`、页缓存三层叠加，极端并发下陈旧读理论可能（单核 baseline 低） | `impl-ext4/src/rw.rs` |

## 8. 收敛建议

1. **BC-P1-01/02**：将 `evict_lru_slot` 的 `expect` 改为返回 `DriverResult<usize>`；不变量失败时 `log::warn!` 打印 `used={map.len()} capacity={} free={} lru={}` 并返回 `DriverError::Internal`（或安全清空缓存后重试一次），**禁止 panic**。
2. **BC-P2-01**：`new` 前可估算 `capacity * block_size`，失败路径由调用方（`wrap`）返回 `Err` 而非依赖堆 panic；日志带 `resource=block-cache-slot`。
3. **BC-P2-02**：LoongArch probe 与 RISC-V 对齐：在 `block-cache` feature 下同样 `BlockCacheManager::wrap`。
4. **BC-P2-03**（可选 debug）：淘汰时 `trace` 级别记录 `evict lba=… idx=…`；或暴露 `CachingBlockDevice::stats()` 供测试。
5. **跨审计**：块设备无 `unregister` 归入 `driver-slots` / 资源 #36；不在本资源单独修。

## 9. 修复任务草案

| 优先级 | 标题 | 文件 | 验收标准 |
|--------|------|------|----------|
| P1 | 块缓存 LRU 淘汰失败改为 Err + warn | `impl-block-cache/src/lib.rs` | 破坏不变量时不 panic；返回明确 `DriverError`；warn 含 used/capacity |
| P1 | `alloc_slot`/`cache_put` 传播淘汰错误 | 同上 + `read_blocks` | `inner.read_blocks` 成功后 `cache_put` 失败时向上返回 Err；已填用户 buf 但缓存未更新（可接受，与读成功一致） |
| P2 | LoongArch 启用 block-cache 包装 | `impl-qemu-loongarch64-virt/src/lib.rs` | `cfg(feature="block-cache")` 下与 riscv 同路径 `BlockCacheManager::wrap` |
| P2 | 启动期 OOM 可诊断 | `manager.rs`、`lib.rs` | `wrap`/`new` 返回 `Result` 或 probe 捕获并 warn，不因 32KiB 槽缓冲无说明 panic |
| P2 | 调试统计钩子（可选） | `lib.rs` | `#[cfg(feature="logging")]` 下淘汰 trace；或单元测试覆盖满容量轮换 |

## 10. 调用链速查

### 读路径（命中）

```
ext4 / VFS / devfs
  → SharedBlockDevice::lock()
  → CachingBlockDevice::read_blocks
  → cache_copy_out → touch_lru
  → (无 inner 访问)
```

### 读路径（未命中）

```
read_blocks → inner.read_blocks(合并区间)
           → cache_put × N → alloc_slot / evict_lru_slot
```

### 写路径

```
write_blocks → inner.write_blocks (先落盘)
            → 若 LBA 已在 map → cache_put 更新槽
```

### 构造与注册

```
VirtioBlkDevice::from_mmio
  → BlockCacheManager::wrap
  → CachingBlockDevice::new (预分配 64 槽)
  → register_block_device
```

## 11. 交叉引用

- 资源清单：`docs/audits/resource-inventory.md` #19
- 锁清单：`docs/audits/lock-inventory.md` #22–#23
- 块设备注册（无注销）：资源 #36 → `driver-slots` subagent
- 页缓存分层：`docs/audits/resources/page-cache.md`（若已产出）
- ext4 小块缓存：`impl-ext4/src/rw.rs` `EXT4_SMALL_READ_CACHE`

---

**账本稳定性总评**：**稳定**  
**P0 摘要**：无确认项；关注 P1 `evict_lru_slot` panic 路径与 LoongArch 未接线（P2）。
