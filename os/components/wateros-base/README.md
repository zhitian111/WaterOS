# wateros-base

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-base` 保存 WaterOS 各组件都可以依赖的最小类型与同步原语。它不实现平台启动、页表、
调度或系统调用；这些语义分别属于 platform、MM、task 和 syscall。同一目录下的 `base-config`
是独立 crate，集中维护跨组件共享的编译期常量。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 导出 `cpu` 与 `sync` 两个模块；不依赖 platform、task、MM 或 syscall，避免基础依赖反向引用上层子系统。 |
| CPU 数据结构 | `src/cpu.rs` | `CpuId`、`CpuMask`、`CpuLocal<T, N>`。 |
| 同步原语 | `src/sync/` | `MultiprocessorSafeCell<T>`、`BootOnceCell<T>`、`RuntimeOnceCell<T>`。 |
| 编译期配置 | `base-config/` | 跨组件共享的编译期常量（MM、task、IPC、FS、klog、syscall 配置）。 |

## 实现说明

- 只保存各组件都可依赖的最小类型与同步原语；不实现平台启动、页表、调度或系统调用。
- `CpuId` 是**逻辑 CPU 编号**，可用于索引 per-CPU 数组；platform 负责把 hart/core 编号映射为
  `CpuId`，scheduler 负责维护 configured/online 状态（online 状态不存放在 base）。
- `CpuMask` 是固定 64 位集合：`EMPTY` / `ALL`（覆盖 `config::task::MAX_CPUS`）、`from_bits` /
  `bits`，`try_from_le_bytes` / `write_le_bytes` 用于 Linux `cpu_set_t` 字节布局。
- `CpuLocal<T, N>` 提供固定容量槽位、不分配堆；`get` 可跨 CPU 共享读取（要求 `T: Sync`），
  `get_local_mut` 是 unsafe 接口，调用方须保证目标槽无并发引用，通常只允许当前 CPU 修改自己
  的槽位。
- `MultiprocessorSafeCell<T>` 是 `spin::Mutex<T>` 的窄封装：推荐 `exclusive_access()` 获取
  guard、`try_lock()` 做不能阻塞的 best-effort 路径。它不代替中断 guard——若同一状态也会在本
  CPU 中断处理程序中访问，调用方必须先关闭本地中断。
- 锁 guard 生命周期内不得：调度/yield/sleep/wait、调用可能再次取得同一把锁的回调、持有
  scheduler/MM/VFS 等上层锁时做不受控的跨层调用。
- `BootOnceCell` 用于 BSP 在开放 AP/运行期消费者前发布对象；`RuntimeOnceCell` 允许多个 CPU
  竞争初始化。两者由同一原子状态机保证内存安全，读取者只在 `Acquire` 观察到完成状态后取得
  `&T`。
- 地址和页号统一由 `wateros-mm` 定义；固件启动参数统一由 `wateros-platform::boot` 定义；
  base 中不再创建第二套地址类型或平台启动上下文。
- `base-config` 只保存编译期常量、不保存可变状态；只有两个以上底层组件必须共享且编译期固定
  的值才放入其中，只属于一个模块的配置应留在该模块。

## 调用链路

依赖方向：

```text
wateros-base-config
        ↓
   wateros-base
        ↓
platform / MM / runtime / task / IPC / VFS / syscall
```

base 不得反向依赖上述上层组件。典型使用：

```text
per-CPU 数组        -> CpuLocal<T, N>（index 用 CpuId）
跨 CPU 共享状态      -> MultiprocessorSafeCell<T>（exclusive_access / try_lock）
一次性发布           -> BootOnceCell / RuntimeOnceCell
编译期共享常量       -> base-config（mm/task/ipc/fs/klog/syscall）
```

## 各实现功能

### src/cpu.rs / CPU 数据结构

- `CpuId`：逻辑 CPU 编号。
- `CpuMask`：固定 64 位 CPU 集合（`EMPTY`/`ALL`/`from_bits`/`bits`/Linux `cpu_set_t` 字节
  布局转换）。
- `CpuLocal<T, N>`：固定容量 per-CPU 槽位，不分配堆；`get` 跨 CPU 共享读需 `T: Sync`，
  `get_local_mut` 仅限当前 CPU 修改自己的槽位。

### src/sync/multiprocessor.rs / 同步原语

- `MultiprocessorSafeCell<T>`：`spin::Mutex<T>` 窄封装；`exclusive_access()` 获取 guard，
  `try_lock()` 实现不可阻塞的 best-effort 路径；持锁期间禁止调度、睡眠、回调重入与不受控
  的跨层调用。

### src/sync/once.rs / 一次性发布

- `BootOnceCell<T>`：BSP 发布、AP/运行期消费者读取（`Release`/`Acquire`）。
- `RuntimeOnceCell<T>`：多 CPU 竞争初始化。

### base-config / 编译期配置

`base-config/src/` 下的文件：

- `mm.rs`：内存布局、堆大小、页等常量。
- `task.rs`：任务/调度容量（如 `MAX_CPUS`）。
- `ipc.rs`：IPC 相关常量。
- `fs.rs`：文件系统常量（如块缓存容量）。
- `klog.rs` / `syscall.rs`：日志与 syscall 相关常量。
- `lib.rs`：模块聚合。

## 修改检查

在本目录运行：

```bash
cargo test --workspace
```

修改公共类型后还应在 `os/` 工作区检查 RISC-V 与 LoongArch profile，确保没有遗留兼容别名或
只在单架构出现的调用点。
