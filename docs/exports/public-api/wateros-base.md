# wateros-base — 公共 API

事实来源：`wateros-base/src/lib.rs`、`wateros-base-config/src/lib.rs`。

## wateros-base（聚合）

### addr

| 类型 | 字段 | 方法 |
|------|------|------|
| `BasePhysAddr` | `val: usize` | `Into<*mut T>` |
| `BaseVirtAddr` | `val: usize` | `Into<*mut T>` |
| `BasePPN` | `val: usize` | — |
| `BaseVPN` | `val: usize` | — |

### boot

- `DTBPA` = `BasePhysAddr`

### cpu

- `CPUHartID` = `usize`

### sync

- `UniprocessorSafeCell<T>`
  - `unsafe fn new(value: T) -> Self`
  - `fn exclusive_access(&self) -> RefMut<'_, T>`

## wateros-base-config（独立 crate，路径 `base-config/`）

根 `wateros` 以 `base_config` 别名依赖；`wateros-abi-api-v0` 等也直接依赖。

| 模块 | 公共常量 / 类型 |
|------|-----------------|
| `syscall` | `MAX_SYSCALL_ARGS` |
| `mm` | `KERNEL_HEAP_SIZE_BIT_WIDTH`、`KERNEL_HEAP_SIZE`、`QEMU_VIRT_PHYS_RAM_*`、`QEMU_VIRT_MMIO_PHYS_*` |
| `ipc` | `DEFAULT_PIPE_CAPACITY` |
| `fs` | `FILE_PAGE_SIZE`、`FILE_LARGE_THRESHOLD`、`FILE_PAGE_CACHE_CAPACITY`、`FILE_READ_AHEAD_STRIDE`、`FileIoMode`、`FILE_IO_MODE`、`BLOCK_CACHE_CAPACITY_BLOCKS` |
| `task` | `SCHED_TIMER_PERIOD_MS`、`MAX_TICKS_PER_TASK`、`READY_QUEUE_STALE_COMPACT_THRESHOLD`、`MAX_RT_TICKS_PER_TASK` |
| `klog` | `KLOG_DESC_SLOTS`、`KLOG_TEXT_RING_BYTES`、`KLOG_MAX_RECORD_BYTES` |

## 设计边界

- `wateros-base` 只放类型，不放配置数值
- 配置数值统一在 `wateros-base-config`，避免双真相
