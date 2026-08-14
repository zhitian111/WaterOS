# mem syscall

本目录管理 brk、mmap VMA、权限、驻留查询和内存策略，实际页表操作由
`wateros-mm::kernel_mm` 在 RISC-V Sv39 与 LoongArch 后端共同实现。

## 当前能力

- brk、mmap/munmap、mprotect、mremap、msync，支持匿名、文件、共享和设备映射。
- fork COW、设备映射 lease、active CPU mask 与 TLB shootdown。
- `MADV_DONTNEED/FREE` 丢弃私有页，`MADV_POPULATE_READ/WRITE` 实际预取页。
- `mincore` 返回每页真实 PTE 驻留位。
- `mlock` 验证并预取区间；当前无 swap/用户页回收，驻留页天然不可换出。
- `MCL_FUTURE`/munlockall 在上述内存模型下成立。

## 已知边界

- `MCL_CURRENT` 缺少全 VMA 枚举公共接口，明确返回 `EOPNOTSUPP`。
- MADV_DONTFORK/DOFORK 会改变 fork 内容，尚未实现时明确失败。
- NUMA policy 当前只提供单节点兼容结果；没有 swap、huge page、userfaultfd。

后续应给 VMA 增加 lock/fork/dump policy 字段、RLIMIT_MEMLOCK 计费和批量 prefault
回滚，再实现 mlock2、完整 mlockall 与 page reclaim 交互。
