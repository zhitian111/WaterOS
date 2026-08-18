# 驱动、存储、网络与运行时跨层开发手册

本文覆盖四条容易跨组件迷路的链：设备从 DTB 到注册表、块设备到根文件系统、网卡到 socket fd，以及 console/heap 等运行时基础设施。详细接口仍以各组件 README 为准；本文强调调用顺序、状态所有者和故障定位。

## 1. 驱动发现不是文件系统挂载

设备启动链为：

```text
main::init_when_boot
  -> driver::init_when_boot（只发布 facade）

main::init_services_after_boot
  -> driver::machine().init_after_boot
       -> 列出 supported-device catalog
       -> enumerate::scan_device_info(DTB)
       -> probe_character_devices
       -> probe_virtio_devices
       -> 注册 block/network/display/input 子系统对象
       -> devfs::sync(unsupported)
  -> network::stack::init
  -> fs::init_when_boot
  -> fs::init_after_boot（探测 FS impl，不挂载）

user_bringup_bus::run
  -> fs::mount_default_root_rw（真正挂根）
```

因此日志中“找到 virtio-blk”只证明设备对象注册成功；“probe matched ext4”只证明文件系统识别成功；只有 `ext4 root mounted (RW)` 才证明全局根卷可用。

## 2. 设备模型与注册表

驱动聚合层入口是 [`components/wateros-driver/src/lib.rs`](../../os/components/wateros-driver/src/lib.rs)。各子系统使用统一模式：

```text
静态 SupportedDeviceEntry
  -> platform enumerate 得到 DeviceInfo
  -> subsystem_claims_device(compatible, type)
  -> 具体设备 from_mmio / 构造
  -> Arc<Mutex<Box<dyn DeviceTrait>>>
  -> register_*_device
  -> 返回稳定 registry index
```

| 层 | 数据 | 所有权 |
| --- | --- | --- |
| `driver-api` | `DeviceInfo`、`MmioRegion`、`MachineDriver` | 无硬件状态的契约 |
| platform `enumerate` | DTB 节点快照 `DEVICE_INFOS` | 当前平台实现 |
| 设备实现 | VirtIO queue、MMIO transport、feature 状态 | 具体 device 对象 |
| 子系统 registry | `Shared*Device` 列表 | block/network/character/display/input |
| devfs | `/dev`、`/dev/sys` 可见节点 | FS/VFS 展示层，不拥有硬件 |

增加设备类型至少需要：API trait、支持目录项、平台识别、实例化与注册、devfs 映射、上层消费者以及自检。只修改 VirtIO device ID 的 match 会得到 `DeviceInfo`，不会自动产生可用设备。

## 3. DTB 与 MMIO 排障

RISC-V 平台的 [`enumerate.rs`](../../os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/enumerate.rs) 遍历全部带 `compatible` 的节点，并解析首个 MMIO region 和 IRQ。VirtIO-MMIO 额外读取：

- offset 0：magic `0x74726976`；
- offset 2：device id，当前识别 net=1、block=2、display=16、input=18。

出现设备缺失时按顺序检查：

1. QEMU argv 是否真的创建设备；
2. DTB 节点是否存在，`compatible`、`reg`、地址 cell 是否解析正确；
3. MMIO magic/device id 是否匹配；
4. `*_subsystem_claims_device` 是否接受该 compatible/type；
5. `from_mmio` 是探测失败还是 queue 初始化失败；
6. registry count 是否增加；
7. devfs 是否刷新。

不要在 MMIO 探测失败时构造“假设备”让后续继续；这会把硬件错误推迟成根挂载、网络超时或数据损坏。

## 4. 重复初始化与并发

平台 driver 用 `INIT_AFTER_BOOT_DONE.swap` 防止重复 bring-up；初始化失败会清回标志以允许重试。设备扫描先构造快照，再在不持有 `DEVICE_INFOS` 锁时进行复杂的设备注册。

修改时遵守：

- 不在 registry/DTB 全局锁内调用可能再次访问 registry 的回调。
- MMIO 寄存器使用 volatile 访问，不能用普通引用缓存。
- 设备对象跨 CPU 共享必须经过其 `Arc<Mutex<...>>`，不能保存裸 `&mut`。
- probe 成功后再提交 registry；失败对象由局部 RAII 回收。
- 重试前应清理上一次已提交的部分状态，或明确把初始化设计成幂等。

## 5. 块设备到根文件系统

文件系统聚合入口是 [`components/wateros-fs/src/lib.rs`](../../os/components/wateros-fs/src/lib.rs)：

```text
block registry
  -> devfs::refresh
  -> default_root_block_path
  -> lookup_block_device
  -> 遍历 registered_fs_impls().probe(device)
  -> 选择支持识别 kind 与 RW/RO mode 的 FsImpl
  -> rootfs::set_active_fs_impl（尚未挂载）
  -> bring-up: mount_default_root_rw
       -> 同一 device 分别 mount_ro 与 mount_rw
       -> ROOT_FS + ROOT_RW_FS + ROOT_DEV_PATH
       -> bump_mount_generation
  -> VFS 根桥接与页缓存
```

根挂载同时保存 RO 与 RW handle：ELF 装载和内核只读路径读取 RO handle，VFS 修改路径使用 RW handle。只保存 RW 会让用户 ELF 装载表现为 `RootVolume(Unsupported)`，即使写文件已可用。

## 6. 文件系统接口分层

| 层 | 负责 | 不负责 |
| --- | --- | --- |
| block driver | 定长扇区 I/O、flush、硬件错误 | inode/path/权限 |
| block cache | 块缓存和向设备 flush | 文件页语义 |
| FS impl | inode、目录、extent、文件 offset、磁盘格式 | per-task fd |
| rootfs | 活动 FS impl、根卷句柄、mount generation | 路径规范化 |
| VFS bridge | path walk、mount route、handle 适配 | ext4 磁盘布局 |
| VFS fd/page cache | fd/OFD offset、CLOEXEC、文件页、writeback | 块设备枚举 |
| syscall | Linux ABI、user copy、errno | 长期文件状态 |

看到 `EIO` 时要保留原始层级：user copy fault 是 `EFAULT`；VFS 不支持操作通常是 `ENOSYS/EOPNOTSUPP`；FS 格式错误与块设备 I/O 才应落到相应 FS/driver 错误。不要把所有后端错误统一成 `ENOENT`。

## 7. 普通文件读写与写回

推荐把 read 实现成准备/复制/提交事务：

```text
sys_read(fd, user_buf, len)
  -> fd table 查 VfsIoHandle
  -> prepare_read / acquire lease
  -> copy_to_user
  -> finish(copied, complete)
  -> 只提交实际复制字节并推进 OFD offset
```

若先推进 offset 再复制，用户缓冲区中途 `EFAULT` 会丢数据。socket 也使用相同 lease 思路。

file-backed shared mmap 的 dirty 页写回链通常是：

```text
PTE/VMA dirty 信息
  -> VFS page cache entry
  -> paged handle writeback(offset, bytes)
  -> FS handle write_at / sync
  -> block cache flush
  -> BlockDevice::flush
```

写回失败日志必须带 aspace/VMA 范围、文件标识、offset/length 与原始错误。销毁地址空间时不应因第一处可报告的写回错误就跳过所有剩余安全清理。

## 8. 添加或替换 FS backend

1. 实现 `FsImpl` 的 supported/probe/mount 接口和 RO/RW handle 契约。
2. 在 FS 聚合层 feature 中保证 backend 互斥。
3. 加入 `registered_fs_impls`，注意匹配顺序决定第一个 probe 命中者。
4. 验证同一块设备的 RO/RW handle 是否允许并存。
5. 实现 metadata、目录变更、truncate、link/unlink/rename、sync/writeback 所需能力。
6. 明确错误映射和部分写语义。
7. 做空镜像/坏 superblock/只读介质/断电式 flush 失败回归。

最小验证不仅是 mount：至少创建、写入、fsync、读回、rename、unlink，再重启用非 snapshot 镜像确认持久化。

## 9. procfs/sysfs 回调边界

procfs 通过函数指针查询 task、VFS、IPC 和 timer 状态。注册回调前对应状态所有者必须完成初始化。读取时推荐：

```text
短暂获取状态锁 -> 复制稳定快照 -> 释放锁 -> 格式化文本 -> 返回
```

禁止持 procfs/FS 锁调用会反向获取 task/IPC/VFS 锁的复杂函数。动态文件是观察接口，不应在 read 中修改被观察状态。

## 10. 网卡到 smoltcp

网络链路：

```text
virtio-net DeviceInfo
  -> VirtioNetDevice::from_mmio
  -> driver network registry
  -> first_network_device
  -> SmoltcpAdapter
  -> Interface + IP/routes + NetworkStack
  -> install_stack
  -> network_poller_task 周期 poll
```

无网卡时 [`stack::init`](../../os/components/wateros-network/network-impl/impl-smoltcp/src/stack/init.rs) 会建立 loopback-only adapter，127.0.0.1 测试仍应工作。外网不通但 loopback 正常时，优先检查设备、QEMU user-net、地址/网关和 poller；loopback 也不通则检查 stack/socket 状态机。

当前默认 IPv4 为 `10.0.2.15/24`，网关 `10.0.2.2`，并安装 `127.0.0.0/8` 路由。prefix 大于 32 返回 invalid argument。

## 11. socket 对象、fd 与最后关闭

[`SocketRef`](../../os/components/wateros-network/src/socket/object.rs) 包装 `Arc<SocketShared>`：

- 底层 `StackSocketHandle` 由 mutex 串行化；
- status flags 是打开文件描述级共享状态；
- `dup`/fork fd 与在途 syscall 都持有 Arc；
- 只有最后一个引用 Drop 时才调用一次 `socket_close`。

VFS handle 的 `close()` 本身不关闭底层 socket。若在每个 fd close 时关闭，dup 后关闭其中一个 fd 会破坏另一个；若在 task exit 只删辅助索引而不释放 fd 引用，则底层 socket 泄漏。

监听 socket 的 accept 会原子地取得 established handle 并用 replacement 替换 listener handle，从而串行化同一 fd 的并发 accept。

## 12. socket 接收事务与阻塞

接收使用 [`SocketReceiveLease`](../../os/components/wateros-network/src/socket/lease.rs)：

```text
poll_snapshot
  -> 分配 staging Vec
  -> socket_prepare_recv（预留、不消费）
  -> copy_to_user
  -> finish(copied, complete)
```

lease 未显式 finish 就 Drop 时，以 `(0, false)` 取消预留。这保证 user copy fault 不吞掉未复制数据。增加 `recvmsg` 控制消息或新协议时继续复用该事务边界，不能绕过后直接消费协议栈 buffer。

阻塞 syscall 不应持有网络全局锁睡眠。正确模式是观察 readiness、释放锁、注册等待/让出 CPU，被 poller 唤醒后重新验证状态；所有 wake 都可能是虚假唤醒。

## 13. runtime 启动与依赖边界

runtime 是其他组件可用前的最小地基：

| 子模块 | 可依赖 | 禁止依赖 |
| --- | --- | --- |
| console | platform early console | VFS、scheduler、heap 回调 |
| logging | console、CPU label | 会分配或递归 logging 的格式化路径 |
| heap allocator | 静态 `.kernel.heap`、arch interrupt | 调度、VFS、logger 回调 |
| panic | console、platform reset | heap、scheduler、VFS 可用性 |
| serial | 已注册 character device | 替代 early console |

日志初始化在 console 可写后且仅由 BSP 执行。panic 是 best-effort：打印、flush、请求关机，不能尝试复杂恢复。

## 14. 内核堆的同步与统计

默认 TLSF，回退实现为 linked-list，二者互斥。堆来自 `KERNEL_HEAP_SIZE` 大小的静态 `.kernel.heap`，只允许 BSP 初始化一次。每次 allocator 调用：

```text
读取当前 CPU 中断状态
  -> 关本 CPU 中断
  -> 增加 CpuLocal 递归深度
  -> 获取 backend 跨 CPU 锁
  -> alloc/dealloc/realloc
  -> 降低深度
  -> 恢复原中断状态
```

allocator guard 内禁止日志格式化分配、调度、VFS 回调或等待。TLSF 的 `used` 只是按请求 layout size 加减的估算，未包含完整 metadata/alignment/碎片开销；所以 `free` 很大但大块分配失败可能是碎片，`used` 回不到原值也可能是调用者泄漏或统计 layout 不配对，必须结合 A/B backend 与对象计数判断。

OOM 日志中的 `layout_size` 是失败请求，不代表单个对象真实业务大小；例如 Vec 扩容可能请求 1 MiB。应从该布局反查增长容器，并同时看持续存活对象数。

## 15. 跨层最小回归矩阵

| 修改 | 最小回归 |
| --- | --- |
| DTB/driver probe | 两架构 check；启动日志含期望 registry count；只读设备自检 |
| block/cache | 多尺寸非对齐读写、flush、重启读回、错误注入 |
| FS backend | mount、元数据、读写、truncate、rename/link/unlink、fsync |
| VFS page/writeback | mmap shared 修改、msync/munmap/exit、重开文件校验 |
| network driver | loopback + 外部 ping/TCP/UDP，SMP 并发 poll |
| socket | dup/fork/close、EFAULT 接收、nonblock、poll、peer shutdown |
| heap | 正常 workload 多轮高水位、TLSF/linked-list A/B、碎片 stress 专用构建 |
| console/log/panic | SMP 日志不拆行；早期 panic 与正常 panic 都能终止 |

回归必须明确是否使用 snapshot。验证磁盘持久化时设置 `WRITE_DISK=1` 并使用可恢复的镜像副本；snapshot 模式只能证明 guest 本次运行内可见，不能证明数据落盘。
