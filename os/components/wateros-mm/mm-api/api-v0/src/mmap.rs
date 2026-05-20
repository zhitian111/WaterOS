//! `mmap`/`munmap` 契约：长度与地址按 **字节** 传入，实现应按 [`crate::addr::PAGE_SIZE`] 向上取整到虚拟页边界。

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
    /// 期望映射起始地址；`None` 表示由实现挑选（须与 `flags`/`kind` 语义一致）。
    pub addr_hint: Option<VirtAddr>,
    /// 映射长度（字节）；实现应按页向上取整到虚拟页边界。
    pub len: usize,
    /// 页级保护（与 [`crate::address_space::AddressSpaceOps::map_page_to_ppn`] 语义对齐）。
    pub prot: PagePerm,
    /// 匿名/私有等 mmap 语义标志。
    pub flags: MapFlags,
    /// 匿名或文件后备等映射种类。
    pub kind: MmapKind,
}

/// mmap / munmap 与地址空间组合的契约；实现须与 [`crate::addr::PAGE_SIZE`] 页粒度一致。
pub trait MmapOps: AddressSpaceOps {
    /// 按请求建立映射；成功返回实际映射起始虚拟地址（可与 `addr_hint` 不同）。
    ///
    /// `file_backing`：仅当 [`MmapKind::File`] 时由 syscall 预读文件内容传入；匿名映射须为 `None`。
    /// 失败时不得留下半映射区间（与具体实现的原子性约定一致）。
    fn mmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        req: MmapRequest,
        file_backing: Option<&[u8]>,
    ) -> MmResult<VirtAddr>;

    /// 解除 `[addr, addr+len)` 语义范围内的映射并回收对应物理帧。
    fn munmap<A: PhysicalFrameAllocator<FrameId = PhysPageNum>>(
        &mut self,
        allocator: &mut A,
        addr: VirtAddr,
        len: usize,
    ) -> MmResult<()>;

    /// 将 `[addr, addr+len)` 内已映射的叶子页权限更新为 `perm`（按页对齐到边界）。
    fn mprotect(&mut self, addr: VirtAddr, len: usize, perm: PagePerm) -> MmResult<()>;
}

