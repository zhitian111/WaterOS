# wateros-base — 已实现功能

事实来源：`os/components/wateros-base/Cargo.toml`、`base-config/Cargo.toml`、`os/Cargo.toml`。

## 用途

提供引导阶段与内核各处共享的薄基础类型与配置常量，避免魔法数在多个 crate 重复定义。

## 聚合 crate（wateros-base）

| 模块 | 状态 | 说明 |
|------|------|------|
| `addr` | 已实现 | `BasePhysAddr`、`BaseVirtAddr`、`BasePPN`、`BaseVPN` 及 `Into<*mut T>` |
| `boot` | 已实现 | `DTBPA` 类型别名 |
| `cpu` | 已实现 | `CPUHartID` 类型别名 |
| `sync` | 已实现 | 单核 `UniprocessorSafeCell<T>`（`RefCell` 包装） |

无 Cargo feature；`#![no_std]`，无外部依赖。

## 子 crate（wateros-base-config）

| 模块 | 状态 | 说明 |
|------|------|------|
| `syscall` | 已实现 | `MAX_SYSCALL_ARGS = 6` |
| `mm` | 已实现 | 内核堆 128MiB、QEMU virt RAM/MMIO 区间常量 |
| `ipc` | 已实现 | `DEFAULT_PIPE_CAPACITY = 4096` |
| `fs` | 已实现 | 页缓存、大文件阈值、块缓存容量、`FileIoMode`（仅 `Direct` 启用） |
| `task` | 已实现 | 定时器周期、时间片、ready 队列 compact 阈值 |
| `klog` | 已实现 | 消息环槽位与单条记录上限 |

## 缺口

- `UniprocessorSafeCell` 仅适用于单核假设，无多 hart 锁原语
- 地址 newtype 不附带对齐或地址空间标识校验
- `FileIoMode::Async` 在配置层存在但 v1 未实现
- QEMU virt 内存常量作 bring-up 回退，真机须以 DTB 为准
