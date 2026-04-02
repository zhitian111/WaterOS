/// 内核错误码（Linux errno 数值的用户态可用形式）
///
/// 约定：系统调用返回错误时，用户态看到的是 `-errno`（通常通过 `isize` 表示）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ErrNo(pub isize);

impl ErrNo {
    #[inline]
    pub const fn raw(self) -> isize {
        self.0
    }

    #[inline]
    pub const fn user_ret(self) -> isize {
        -self.0
    }
}

// Linux errno（riscv64 与各架构通用）
impl ErrNo {
    pub const EPERM: Self = Self(1);
    pub const ENOENT: Self = Self(2);
    pub const ESRCH: Self = Self(3);
    pub const EINTR: Self = Self(4);
    pub const EIO: Self = Self(5);
    pub const EBADF: Self = Self(9);
    pub const ECHILD: Self = Self(10);
    pub const EAGAIN: Self = Self(11);
    pub const ENOMEM: Self = Self(12);
    pub const EACCES: Self = Self(13);
    pub const EFAULT: Self = Self(14);
    pub const EBUSY: Self = Self(16);
    pub const EEXIST: Self = Self(17);
    pub const EINVAL: Self = Self(22);
    pub const ENOSYS: Self = Self(38);
    pub const ENOTDIR: Self = Self(20);
    pub const EPIPE: Self = Self(32);
}

/// 常用的内核返回值（避免在 syscall 层到处写魔数）
pub type KernelResult<T> = core::result::Result<T, ErrNo>;
