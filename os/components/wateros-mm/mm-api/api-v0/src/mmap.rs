//! `mmap`/`munmap` 契约：长度与地址按 **字节** 传入，实现应按 [`crate::addr::PAGE_SIZE`] 向上取整到虚拟页边界。

extern crate alloc;

use alloc::{boxed::Box, sync::Arc};

use crate::addr::{PhysPageNum, VirtAddr};
use crate::address_space::AddressSpaceOps;
use crate::error::MmResult;
use crate::flags::MapFlags;
use crate::frame_allocator::PhysicalFrameAllocator;
use crate::perm::PagePerm;

/// mmap 映射类型（先做最小集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmapKind {
    /// 匿名映射：内容来自零填充（当前阶段可先按需延后实现）。
    Anonymous,
    /// 文件映射占位：供后续 VFS/文件后备填充。
    File { fd : usize, offset : usize },
    /// 外部设备物理页映射；页的所有权不属于地址空间。
    Device { offset : usize },
}

/// 设备映射生命周期令牌。
///
/// MM 层不依赖具体驱动；VMA 持有该对象即可防止底层 DMA
/// 缓冲在用户映射存活期间被释放。
pub trait DeviceMappingLease: Send + Sync {}

impl<T : Send + Sync + ?Sized> DeviceMappingLease for T {}

/// 一段物理连续、不由通用帧分配器回收的设备内存。
#[derive(Clone)]
pub struct DeviceMapping {
    pub phys_start : PhysPageNum,
    pub len : usize,
    pub lease : Arc<dyn DeviceMappingLease>,
}

/// mmap 请求结构（从 syscall/ABI 层组装）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmapRequest {
    /// 期望映射起始地址；`None` 表示由实现挑选（须与 `flags`/`kind` 语义一致）。
    pub addr_hint : Option<VirtAddr>,
    /// 映射长度（字节）；实现应按页向上取整到虚拟页边界。
    pub len : usize,
    /// 页级保护（与 [`crate::address_space::AddressSpaceOps::map_page_to_ppn`] 语义对齐）。
    pub prot : PagePerm,
    /// 匿名/私有等 mmap 语义标志。
    pub flags : MapFlags,
    /// 匿名或文件后备等映射种类。
    pub kind : MmapKind,
}

/// Demand paging 的 fault 类型，用于权限检查与按需装页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFaultAccess {
    Read,
    Write,
    Execute,
}

/// 文件页懒加载器。实现者须自行持有 mmap 后仍可读取文件内容的状态。
pub trait DemandPageLoader {
    /// 复制 loader；用于 fork 后父子地址空间都保留同一文件映射语义。
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>>;

    /// 将文件偏移 `file_offset` 对应的一页加载到已清零的 `dst`。
    fn load_page(&mut self, file_offset : usize, dst : &mut [u8]) -> MmResult<()>;

    /// Optionally return an immutable physical page already populated for this
    /// file offset. A returned PPN owns one reference for the caller's mapping;
    /// the caller must release it if page-table installation fails. Writable
    /// loaders and loaders without a shared cache retain the default `None`.
    fn load_shared_page(&mut self, _file_offset : usize) -> MmResult<Option<PhysPageNum>> {
        Ok(None)
    }

    /// 将共享映射页写回文件后备。只读 loader 可保留默认的不支持实现。
    fn write_page(&mut self, _file_offset : usize, _src : &[u8]) -> MmResult<()> {
        Err(crate::error::MmError::Unsupported)
    }

    /// 提交此前的共享映射写回。
    fn flush(&mut self) -> MmResult<()> { Ok(()) }
}

/// mmap / munmap 与地址空间组合的契约；实现须与 [`crate::addr::PAGE_SIZE`] 页粒度一致。
pub trait MmapOps: AddressSpaceOps {
    /// 按请求建立映射；成功返回实际映射起始虚拟地址（可与 `addr_hint` 不同）。
    ///
    /// `file_backing`：仅当 [`MmapKind::File`] 时由 syscall 预读文件内容传入；匿名映射须为 `None`。
    /// 失败时不得留下半映射区间（与具体实现的原子性约定一致）。
    fn mmap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                               allocator : &mut A,
                                                               req : MmapRequest,
                                                               file_backing : Option<&[u8]>)
                                                               -> MmResult<VirtAddr>;

    /// 建立 eager 共享文件映射，并保留 loader 供 `msync`/`munmap`/销毁时写回。
    fn mmap_file_shared<A>(&mut self,
                           allocator : &mut A,
                           req : MmapRequest,
                           loader : Box<dyn DemandPageLoader>)
                           -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>;

    /// 登记文件懒映射；成功返回实际映射起始虚拟地址。实现不得在该调用中读取整段文件。
    fn mmap_file_lazy<A>(&mut self,
                         allocator : &mut A,
                         req : MmapRequest,
                         file_size : usize,
                         loader : Box<dyn DemandPageLoader>)
                         -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>;

    /// 将外部设备页 eager 映射到用户空间。
    ///
    /// 实现必须保留 `mapping.lease`，且解除映射时只删除 PTE，
    /// 不得把 `phys_start` 对应物理页交给通用帧分配器。
    fn mmap_device<A>(&mut self,
                      allocator : &mut A,
                      req : MmapRequest,
                      mapping : DeviceMapping)
                      -> MmResult<VirtAddr>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>;

    /// 处理用户页故障。返回 `Ok(true)` 表示已装入/修复该页，可重试用户访问。
    fn handle_page_fault<A>(&mut self,
                            allocator : &mut A,
                            fault_addr : VirtAddr,
                            access : PageFaultAccess)
                            -> MmResult<bool>
        where A : PhysicalFrameAllocator<FrameId = PhysPageNum>;

    /// 解除 `[addr, addr+len)` 语义范围内的映射并回收对应物理帧。
    fn munmap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 addr : VirtAddr,
                                                                 len : usize)
                                                                 -> MmResult<()>;

    /// 解除一段由地址空间外部对象持有的物理页映射。
    ///
    /// 该接口用于 SysV SHM 等“物理页由独立注册表管理”的映射。实现只能删除
    /// PTE 和对应 VMA 元数据，绝不能把叶子 PPN 交给通用帧分配器。调用方必须
    /// 在确认该范围确实属于外部对象后使用它，并负责外部对象最终的生命周期。
    fn munmap_external(&mut self, addr : VirtAddr, len : usize) -> MmResult<()>;

    /// 将范围内的可写共享文件映射同步到其文件后备。
    fn msync(&mut self, addr : VirtAddr, len : usize) -> MmResult<()>;

    /// 将 `[addr, addr+len)` 内已映射的叶子页权限更新为 `perm`（按页对齐到边界）。
    ///
    /// 返回值表示是否至少有一个驻留叶 PTE（含其 PPN）实际发生变化；只更新 lazy VMA
    /// 元数据时返回 `false`，供调用方避免不必要的 TLB shootdown。
    fn mprotect(&mut self, addr : VirtAddr, len : usize, perm : PagePerm) -> MmResult<bool>;

    /// 调整已有映射大小或地址（Linux `mremap(2)` 语义子集）。
    fn mremap<A : PhysicalFrameAllocator<FrameId = PhysPageNum>>(&mut self,
                                                                 allocator : &mut A,
                                                                 old_addr : VirtAddr,
                                                                 old_size : usize,
                                                                 new_size : usize,
                                                                 flags : usize,
                                                                 new_address : VirtAddr)
                                                                 -> MmResult<VirtAddr>;
}
