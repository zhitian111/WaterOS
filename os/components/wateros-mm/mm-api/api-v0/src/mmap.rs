//! `mmap`/`munmap` 契约：长度与地址按 **字节** 传入，实现应按 [`crate::addr::PAGE_SIZE`] 向上取整到虚拟页边界。

extern crate alloc;

use alloc::boxed::Box;

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

/// Stable identity used to invalidate every resident mapping of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileObjectId {
    pub mount_id : u64,
    pub inode_id : u64,
}

/// Page-cache pin owned by a shared-file-page lease.
///
/// Dropping the token cancels an uncommitted lease. `commit_mapping` converts
/// the short pin into one long-lived mapping reference after the PTE install.
pub trait SharedFilePagePin {
    fn commit_mapping(self : Box<Self>) -> MmResult<Box<dyn SharedFileMappingRef>>;
}

/// Long-lived cache mapping reference retained alongside an installed PTE.
pub trait SharedFileMappingRef: Send {
    fn duplicate_box(&self) -> MmResult<Box<dyn SharedFileMappingRef>>;
}

/// A clean, generation-stable physical file page borrowed from the VFS cache.
pub struct SharedFilePageLease {
    pub ppn : PhysPageNum,
    pub file_id : FileObjectId,
    pub page_index : usize,
    pub generation : u64,
    pub valid_len : usize,
    pin : Option<Box<dyn SharedFilePagePin>>,
}

impl SharedFilePageLease {
    pub fn new(ppn : PhysPageNum,
               file_id : FileObjectId,
               page_index : usize,
               generation : u64,
               valid_len : usize,
               pin : Box<dyn SharedFilePagePin>)
               -> Self {
        Self { ppn,
               file_id,
               page_index,
               generation,
               valid_len,
               pin : Some(pin) }
    }

    /// Commit the cache pin after the PTE and reverse-map entry are installed.
    pub fn commit_mapping(mut self) -> MmResult<Box<dyn SharedFileMappingRef>> {
        self.pin.take().expect("shared file page lease without pin").commit_mapping()
    }
}

/// Result of asking a loader for a directly mappable page.
pub enum DemandPage {
    SharedReadOnly(SharedFilePageLease),
    CopyRequired,
}

/// 文件页懒加载器。实现者须自行持有 mmap 后仍可读取文件内容的状态。
pub trait DemandPageLoader {
    /// 复制 loader；用于 fork 后父子地址空间都保留同一文件映射语义。
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>>;

    /// Acquire a directly mappable page or explicitly select the copy path.
    fn acquire_page(&mut self,
                    file_offset : usize,
                    access : PageFaultAccess)
                    -> MmResult<DemandPage>;

    /// 将文件偏移 `file_offset` 对应的一页加载到已清零的 `dst`。
    fn load_page(&mut self, file_offset : usize, dst : &mut [u8]) -> MmResult<()>;

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

    /// 将范围内的可写共享文件映射同步到其文件后备。
    fn msync(&mut self, addr : VirtAddr, len : usize) -> MmResult<()>;

    /// 将 `[addr, addr+len)` 内已映射的叶子页权限更新为 `perm`（按页对齐到边界）。
    fn mprotect(&mut self, addr : VirtAddr, len : usize, perm : PagePerm) -> MmResult<()>;

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
