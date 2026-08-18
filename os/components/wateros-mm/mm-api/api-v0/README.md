# MM API v0 开发手册

[MM 总览](../../README.md) · [离线开发手册](../../../../docs/offline-development/README.md)

本 crate 是物理内存、地址空间实现与 task/syscall 之间的稳定契约层。它只描述“地址、权限、映射和用户访问应当具有什么语义”，不编码 Sv39/LoongArch64 PTE，也不持有 VFS 文件或 Linux errno。新增跨架构内存能力时，优先在这里定义最小 trait/数据结构，再分别实现两个架构。

## 文件与职责

| 文件 | 主要类型 | 修改场景 |
| --- | --- | --- |
| `addr.rs` | `VirtAddr`、`PhysAddr`、`VirtPageNum`、`PhysPageNum` | 地址换算、4 KiB 页对齐和页号索引。 |
| `perm.rs`、`flags.rs` | `PagePerm`、`MapFlags` | 页权限和 mmap 行为标志；不要直接放架构 PTE 位。 |
| `address_space.rs` | `AddressSpaceId`、`AddressSpaceOps` | 单页 map/unmap/translate 等最小页表操作。 |
| `mmap.rs` | `MmapRequest`、`MmapKind`、`MmapOps`、`DemandPageLoader` | 匿名/文件/设备映射、fault、`mprotect`、`mremap`、`msync`。 |
| `brk.rs` | `BrkRegion`、`HeapBrk` | 进程堆边界和扩缩容契约。 |
| `user_access.rs` | `UserMemoryOps`、`UserCopyProgress`、`FutexMappingIdentity` | 跨页拷贝、用户原子操作和 futex 身份。 |
| `user_aspace_lifecycle.rs` | drop、CPU enter/leave 钩子 | task 不依赖具体 MM 实现即可管理地址空间。 |
| `user_mapping.rs` | `UserMappingSnapshot` | `/proc` 等只读观察者取得映射快照。 |
| `elf_user_stack.rs`、`executable.rs` | ELF 初始栈、shebang/ELF 判别 | exec 装载前的架构无关规则。 |
| `kernel_bringup.rs`、`kernel_satp.rs` | 启动错误与内核页表 token | 内核 MM 启动和 AP 激活边界。 |

## 必须保持的契约

- 页大小固定为 `PAGE_SIZE = 4096`。页区间使用半开形式 `[start, end)`；字节长度先检查加法溢出，再用 `floor_page`/`ceil_page` 对齐。
- `MmapRequest.len`、文件 `offset` 和 `mremap` 大小均为字节，不是页数。
- `PteChange::Changed` 只表示驻留叶 PTE 发生变化；只更新 lazy VMA 元数据应返回 `None`，避免无意义的 TLB shootdown。
- `DemandPageLoader::load_shared_page` 返回的 PPN 已为调用者持有一个引用；PTE 安装失败时实现必须释放该引用。
- `DeviceMapping` 的页归驱动所有，VMA 只持有 `lease` 保活。`munmap`/destroy 只能删 PTE，不能回收到普通帧池。
- `munmap_external` 用于 SysV SHM 等外部所有者。调用者必须先证明目标区间确属该对象。
- `UserCopyProgress.completed` 是错误发生前已完成的精确前缀，不能在跨页失败后简单返回零。
- futex 的共享身份必须稳定；私有或可能 COW 的映射返回 `Private`，不能把会变化的物理页号当共享 key。

## 典型调用链

```text
sys_mmap
  -> 将 Linux prot/flags/fd 转成 MmapRequest + DemandPageLoader
  -> 当前 task 取得 user_aspace_ptr
  -> with_user_aspace_mut_and_flush...
  -> MmapOps::{mmap_file_lazy,mmap_device,mmap}
  -> VMA/PTE 改动
  -> 按 PteChange 做本地及远端 TLB 失效
```

```text
task context switch
  -> notify_aspace_cpu_leave(old_handle, cpu)
  -> 激活新页表 token
  -> notify_aspace_cpu_enter(new_handle, cpu)

task exit
  -> drop_user_aspace_on_task_exit(handle)
  -> 架构实现注册的 destroy(handle)
  -> 写回共享映射、释放普通映射页/页表/ASID
```

生命周期钩子未注册时是 no-op，便于 dummy/早期启动，但正式用户任务运行前 `mm::kernel_mm::init` 必须完成注册。一个非零 raw handle 只能销毁一次，并且销毁前不能再被任何 CPU 使用。

## 新增内存 syscall 的落地模板

以增加一个修改映射属性的 syscall 为例：

1. 判断该能力是否需要新的跨架构语义；需要时在 `MmapOps` 添加返回 `MmResult` 的方法，不在 API 层返回 `-errno`。
2. 明确输入单位、对齐、空区间、溢出、部分成功和 PTE 是否变化。
3. 在 `impl-sv39` 与 `impl-loongarch64` 同时实现；公共 VMA 算法放到 `mm-impl/common`。
4. syscall 层只负责 ABI 转换、fd/文件所有权和 `MmError -> errno`。
5. 所有修改驻留 PTE 的入口经 `with_user_aspace_mut_and_flush*`；不能在 syscall 里直接取得页表锁后绕过 flush。
6. 为未驻留 VMA、已驻留 PTE、跨页、权限失败、SMP 和退出回收分别补测试。

## 回归检查

- `cargo check` 必须覆盖 `impl-sv39` 和 `impl-loongarch64` 两条 feature 链。
- 地址/权限等纯契约运行 `mm_api::test()`。
- 新 trait 方法应有两个真实架构实现，搜索 trait 名确认没有遗漏 dummy/测试实现。
- mmap 修改至少验证匿名、私有文件、共享文件、设备、外部页五种所有权路径。
- 错误路径检查新分配帧、共享页引用、loader 和 lease 是否全部回滚。
- 在 SMP 压力下观察 `[tlb]` 超时和地址空间销毁后的 stale handle；单核通过不能替代该项。
