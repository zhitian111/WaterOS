//! 用户任务规格与资源快照：地址空间句柄占位、映像区间与外部用户栈选项，供
//! `impl-core` 装配 TCB。
//!
//! `AddressSpaceHandle` 仅为稳定 ABI 形状；具体 MMU 地址空间 token
//! 绑定在平台与后续 MM 子系统中完成。


/// 用户态程序入口 PC。
pub type UserTaskEntryPc = usize;
/// 预留给后续地址空间实现使用的稳定句柄占位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpaceHandle {
    raw : usize,
}

impl AddressSpaceHandle {
    /// 基于一个实现自定义的原始值构造地址空间句柄。
    #[inline]
    pub const fn from_raw(raw : usize) -> Self { Self { raw } }

    /// 读取该句柄对应的原始值。
    #[inline]
    pub const fn raw(self) -> usize { self.raw }
}

/// 用户任务关联的一段用户映像元信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserImageInfo {
    image_base : usize,
    image_size : usize,
}

impl UserImageInfo {
    /// 基于映像起始地址和大小构造一份元信息。
    #[inline]
    pub const fn new(image_base : usize, image_size : usize) -> Self {
        Self { image_base,
               image_size }
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
pub struct UserTask {
    entry_pc : UserTaskEntryPc,
    address_space : Option<AddressSpaceHandle>,
    image : Option<UserImageInfo>,
    stack : Option<UserStack>,
    /// Sv39 用户页表对象指针（`impl-sv39` 下为 `&mut Sv39AddressSpace`
    /// 泄漏地址）；无则为 `None`。
    user_aspace_ptr : Option<usize>,
    /// 首次 `sret` 进入用户态时的栈指针；`None` 时使用栈顶减 16 字节空栈。
    initial_user_sp : Option<usize>,
    /// 首次进入用户态时传给 C runtime 的入口参数；具体架构决定是否写寄存器。
    initial_user_args : Option<(usize, usize, usize)>,
}

impl UserTask {
    /// 基于用户入口地址构造一份最小任务规格。
    #[inline]
    pub const fn new(entry_pc : UserTaskEntryPc,
                     address_space : AddressSpaceHandle,
                     image : UserImageInfo,
                     stack : UserStack,
                     aspace_ptr : usize)
                     -> Self {
        Self { entry_pc,
               address_space : Some(address_space),
               image : Some(image),
               stack : Some(stack),
               user_aspace_ptr : Some(aspace_ptr),
               initial_user_sp : None,
               initial_user_args : None }
    }

    /// 指定首次进入用户态时的栈指针（须已由 MM 写入 argc/argv 等）。
    #[inline]
    pub const fn with_initial_user_sp(self, sp : usize) -> Self {
        Self { initial_user_sp : Some(sp),
               ..self }
    }

    /// 指定首次进入用户态时的 argc/argv/envp 入口参数。
    #[inline]
    pub const fn with_initial_user_args(self, argc : usize, argv : usize, envp : usize) -> Self {
        Self { initial_user_args : Some((argc, argv, envp)),
               ..self }
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

    /// 若已指定外部用户栈区间，则返回 `(bottom, top)`。
    #[inline]
    pub const fn stack(&self) -> Option<UserStack> { self.stack }

    /// 若已指定 Sv39 用户页表对象指针，则返回其裸地址。
    #[inline]
    pub const fn user_aspace_ptr(&self) -> Option<usize> { self.user_aspace_ptr }

    /// 退出后丢弃地址空间句柄（页表已在 `exit` 时销毁）。
    #[inline]
    pub const fn without_user_aspace(self) -> Self {
        Self { address_space : None,
               user_aspace_ptr : None,
               ..self }
    }

    /// 若已指定首次用户栈指针，则返回该值。
    #[inline]
    pub const fn initial_user_sp(&self) -> Option<usize> { self.initial_user_sp }

    /// 若已指定首次用户态入口参数，则返回 `(argc, argv, envp)`。
    #[inline]
    pub const fn initial_user_args(&self) -> Option<(usize, usize, usize)> {
        self.initial_user_args
    }
}

/// 用户任务的栈封装：记录外部（MM ELF loader）已映射的虚拟地址区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserStack {
    bottom : usize,
    top : usize,
    size : usize,
}

impl UserStack {
    /// 基于外部已映射的虚拟地址区间构造用户栈。
    #[inline]
    pub const fn from_range(bottom : usize, top : usize) -> Self {
        Self { bottom,
               top,
               size : top.saturating_sub(bottom) }
    }

    #[inline]
    /// 返回当前用户栈的栈底地址。
    pub fn bottom(&self) -> usize { self.bottom }

    #[inline]
    /// 返回当前用户栈的栈顶地址。
    pub fn top(&self) -> usize { self.top }

    #[inline]
    /// 返回当前用户栈大小。
    pub fn size(&self) -> usize { self.size }
}
