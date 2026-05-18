//! 用户任务规格与资源快照：地址空间句柄占位、映像区间与外部用户栈选项，供 `impl-core` 装配 TCB。
//!
//! `AddressSpaceHandle` 仅为稳定 ABI 形状；具体 MMU/`satp` 绑定在平台与后续 MM 子系统中完成。

use crate::task::UserTaskEntryPc;

/// 预留给后续地址空间实现使用的稳定句柄占位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpaceHandle {
    raw: usize,
}

impl AddressSpaceHandle {
    /// 基于一个实现自定义的原始值构造地址空间句柄。
    #[inline]
    pub const fn from_raw(raw: usize) -> Self {
        Self { raw }
    }

    /// 读取该句柄对应的原始值。
    #[inline]
    pub const fn raw(self) -> usize {
        self.raw
    }
}

/// 用户任务关联的一段用户映像元信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserImageInfo {
    image_base: usize,
    image_size: usize,
}

impl UserImageInfo {
    /// 基于映像起始地址和大小构造一份元信息。
    #[inline]
    pub const fn new(image_base: usize, image_size: usize) -> Self {
        Self {
            image_base,
            image_size,
        }
    }

    /// 返回映像起始地址。
    #[inline]
    pub const fn image_base(&self) -> usize {
        self.image_base
    }

    /// 返回映像大小。
    #[inline]
    pub const fn image_size(&self) -> usize {
        self.image_size
    }
}

/// 创建用户任务时需要提供的最小启动规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserTaskSpec {
    entry_pc: UserTaskEntryPc,
    address_space: Option<AddressSpaceHandle>,
    image: Option<UserImageInfo>,
    /// 已由 MM 映射好的用户栈 `(bottom, top]`；若 `None` 则使用内核侧 `UserStack` 分配。
    external_stack: Option<(usize, usize)>,
    /// Sv39 用户页表对象指针（`impl-sv39` 下为 `&mut Sv39AddressSpace` 泄漏地址）；无则为 `None`。
    user_aspace_ptr: Option<usize>,
}

impl UserTaskSpec {
    /// 基于用户入口地址构造一份最小任务规格。
    #[inline]
    pub const fn new(entry_pc: UserTaskEntryPc) -> Self {
        Self {
            entry_pc,
            address_space: None,
            image: None,
            external_stack: None,
            user_aspace_ptr: None,
        }
    }

    /// 为该用户任务规格附上一份地址空间句柄占位。
    #[inline]
    pub const fn with_address_space(self, address_space: AddressSpaceHandle) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: Some(address_space),
            image: self.image,
            external_stack: self.external_stack,
            user_aspace_ptr: self.user_aspace_ptr,
        }
    }

    /// 为该用户任务规格附上一份用户映像元信息。
    #[inline]
    pub const fn with_image(self, image: UserImageInfo) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: self.address_space,
            image: Some(image),
            external_stack: self.external_stack,
            user_aspace_ptr: self.user_aspace_ptr,
        }
    }

    /// 指定已由 MMU 映射的用户栈虚拟地址区间 `(bottom, top]`。
    #[inline]
    pub const fn with_external_stack(self, bottom: usize, top: usize) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: self.address_space,
            image: self.image,
            external_stack: Some((bottom, top)),
            user_aspace_ptr: self.user_aspace_ptr,
        }
    }

    /// 附加 MM 提供的用户地址空间对象指针（供 `brk`/`mmap` 等 syscall 修改页表）。
    #[inline]
    pub const fn with_user_aspace_ptr(self, ptr: usize) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: self.address_space,
            image: self.image,
            external_stack: self.external_stack,
            user_aspace_ptr: Some(ptr),
        }
    }

    /// 返回用户态首次进入时的目标 PC。
    #[inline]
    pub const fn entry_pc(&self) -> UserTaskEntryPc {
        self.entry_pc
    }

    /// 返回当前规格附带的地址空间句柄占位。
    #[inline]
    pub const fn address_space(&self) -> Option<AddressSpaceHandle> {
        self.address_space
    }

    /// 返回当前规格附带的用户映像元信息。
    #[inline]
    pub const fn image(&self) -> Option<UserImageInfo> {
        self.image
    }

    /// 若已指定外部用户栈区间，则返回 `(bottom, top)`。
    #[inline]
    pub const fn external_stack(&self) -> Option<(usize, usize)> {
        self.external_stack
    }

    /// 若已指定 Sv39 用户页表对象指针，则返回其裸地址。
    #[inline]
    pub const fn user_aspace_ptr(&self) -> Option<usize> {
        self.user_aspace_ptr
    }
}

/// 对外暴露的用户任务资源快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserTaskResources {
    /// 用户任务首次返回用户态时的入口 PC。
    pub entry_pc: UserTaskEntryPc,
    /// 当前用户栈底地址。
    pub user_stack_bottom: usize,
    /// 当前用户栈顶地址。
    pub user_stack_top: usize,
    /// 当前用户栈大小。
    pub user_stack_size: usize,
    /// 预留给后续地址空间/loader 使用的地址空间句柄占位。
    pub address_space: Option<AddressSpaceHandle>,
    /// 若创建时已知用户映像信息，则在这里保留稳定快照。
    pub image: Option<UserImageInfo>,
    /// 用户 Sv39 页表对象指针（`wateros-mm` `impl-sv39`）；`0` 表示无。
    pub user_aspace_ptr: usize,
}
