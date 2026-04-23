use crate::task::UserTaskEntryPc;

/// 预留给后续地址空间实现使用的稳定句柄占位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpaceHandle {
    raw: usize,
}

impl AddressSpaceHandle {
    /// 基于一个实现自定义的原始值构造地址空间句柄。
    #[inline]
    pub const fn from_raw(raw: usize) -> Self { Self { raw } }

    /// 读取该句柄对应的原始值。
    #[inline]
    pub const fn raw(self) -> usize { self.raw }
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
    pub const fn image_base(&self) -> usize { self.image_base }

    /// 返回映像大小。
    #[inline]
    pub const fn image_size(&self) -> usize { self.image_size }
}

/// 创建用户任务时需要提供的最小启动规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserTaskSpec {
    entry_pc: UserTaskEntryPc,
    address_space: Option<AddressSpaceHandle>,
    image: Option<UserImageInfo>,
}

impl UserTaskSpec {
    /// 基于用户入口地址构造一份最小任务规格。
    #[inline]
    pub const fn new(entry_pc: UserTaskEntryPc) -> Self {
        Self {
            entry_pc,
            address_space: None,
            image: None,
        }
    }

    /// 为该用户任务规格附上一份地址空间句柄占位。
    #[inline]
    pub const fn with_address_space(self, address_space: AddressSpaceHandle) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: Some(address_space),
            image: self.image,
        }
    }

    /// 为该用户任务规格附上一份用户映像元信息。
    #[inline]
    pub const fn with_image(self, image: UserImageInfo) -> Self {
        Self {
            entry_pc: self.entry_pc,
            address_space: self.address_space,
            image: Some(image),
        }
    }

    /// 返回用户态首次进入时的目标 PC。
    #[inline]
    pub const fn entry_pc(&self) -> UserTaskEntryPc { self.entry_pc }

    /// 返回当前规格附带的地址空间句柄占位。
    #[inline]
    pub const fn address_space(&self) -> Option<AddressSpaceHandle> { self.address_space }

    /// 返回当前规格附带的用户映像元信息。
    #[inline]
    pub const fn image(&self) -> Option<UserImageInfo> { self.image }
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
}
