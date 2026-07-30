# wateros-base

`wateros-base` 保存 WaterOS 各组件都可以依赖的最小类型与同步原语。它不实现
平台启动、页表、调度或系统调用；这些语义分别属于 platform、MM、task 和 syscall。

同一目录下的 `base-config` 是独立 crate，集中维护跨组件共享的编译期常量。

## 目录与职责

| 路径 | 主要内容 | 关键约束 |
|---|---|---|
| `src/cpu.rs` | `CpuId`、`CpuMask`、`CpuLocal<T, N>` | 逻辑 CPU ID 不等于硬件 ID；online 状态不存放在 base |
| `src/sync/multiprocessor.rs` | `MultiprocessorSafeCell<T>` | 自旋互斥，不关闭中断、不可重入、不可持锁调度或睡眠 |
| `src/sync/once.rs` | `BootOnceCell<T>`、`RuntimeOnceCell<T>` | 单次写入，`Release/Acquire` 发布后无锁读取 |
| `base-config/src` | MM、task、IPC、FS、klog、syscall 配置 | 只保存编译期常量，不保存可变状态 |

地址和页号统一由 `wateros-mm` 定义；固件启动参数统一由 `wateros-platform::boot`
定义。不要在 base 中再创建第二套地址类型或平台启动上下文。

## CPU 数据结构

`CpuId` 是逻辑 CPU 编号，可用于索引 per-CPU 数组。platform 负责把 hart/core
编号映射为 `CpuId`，scheduler 负责维护 configured/online 状态。

`CpuMask` 是固定 64 位集合：

- `EMPTY` 不包含 CPU；
- `ALL` 包含 `config::task::MAX_CPUS` 范围内的 CPU；
- `from_bits`/`bits` 用于内核接口；
- `try_from_le_bytes`/`write_le_bytes` 用于 Linux `cpu_set_t` 字节布局。

`CpuLocal<T, N>` 提供固定容量槽位，不分配堆。`get` 可以跨 CPU 共享读取，因此
`CpuLocal` 只有在 `T: Sync` 时才可跨 CPU 共享；`get_local_mut` 是 unsafe 接口，
调用方必须保证目标槽没有任何并发引用，通常只允许当前 CPU 修改自己的槽位。

## 同步数据结构

`MultiprocessorSafeCell<T>` 是 `spin::Mutex<T>` 的窄封装。推荐用
`exclusive_access()` 获取 guard，用 `try_lock()` 实现不能阻塞的 best-effort
路径。它不代替中断 guard：若同一状态也会在本 CPU 的中断处理程序中访问，调用方
必须先关闭本地中断。

锁 guard 离开作用域后才会释放，因此以下操作不得发生在 guard 生命周期内：

- 调度、yield、sleep 或 wait；
- 调用可能再次取得同一把锁的回调；
- 持有 scheduler/MM/VFS 等上层锁时进行不受控的跨层调用。

`BootOnceCell<T>` 用于 BSP 在开放 AP/运行期消费者前发布对象；
`RuntimeOnceCell<T>` 允许多个 CPU 竞争初始化。两者均由同一个原子状态机保证
内存安全，区别是生命周期语义。读取者只在 `Acquire` 观察到完成状态后取得 `&T`。

## 依赖方向

```text
wateros-base-config
        ↓
   wateros-base
        ↓
platform / MM / runtime / task / IPC / VFS / syscall
```

base 不得反向依赖上述上层组件。配置常量若只属于一个模块，应留在该模块；只有
两个以上底层组件必须共享且编译期固定的值才放入 `base-config`。

## 修改检查

在本目录运行：

```bash
cargo test --workspace
```

修改公共类型后还应在 `os/` 工作区检查 RISC-V 与 LoongArch profile，确保没有
遗留兼容别名或只在单架构出现的调用点。
