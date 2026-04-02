use crate::address_space::AddressSpaceOps;
use crate::addr::{VirtAddr, PhysPageNum};
use crate::error::MmResult;
use crate::frame_allocator::PhysicalFrameAllocator;
use crate::flags::MapFlags;
use crate::perm::PagePerm;

/// mmap 映射类型（先做最小集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmapKind {
    /// 匿名映射：内容来自零填充（当前阶段可先按需延后实现）。
    Anonymous,
    /// 文件映射占位：供后续 VFS/文件后备填充。
    File { fd: usize, offset: usize },
}

/// mmap 请求结构（从 syscall/ABI 层组装）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmapRequest {
    pub addr_hint: Option<VirtAddr>,
    pub len: usize,
    pub prot: PagePerm,
    pub flags: MapFlags,
    pub kind: MmapKind,
}

/// mmap/unmap 契约（先占位即可）。
pub trait MmapOps: AddressSpaceOps {
    /// 创建映射并返回映射起始虚拟地址。
    fn mmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        req: MmapRequest,
    ) -> MmResult<VirtAddr>;

    /// 删除映射并回收帧。
    fn munmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        addr: VirtAddr,
        len: usize,
    ) -> MmResult<()>;
}

