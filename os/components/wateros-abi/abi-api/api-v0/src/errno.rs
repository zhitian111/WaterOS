//! Linux 风格错误码及其与系统调用返回值的对应关系。
//!
//! 数值与 Linux errno 一致；与 [`crate::user_ret::UserRet`] 组合时，错误路径使用负值表示。
//!
//! English: mirrors Linux errno integers; failures surface to userspace as negative
//! values paired with [`crate::user_ret::UserRet`].

/// 内核错误码（Linux errno 数值的用户态可用形式）
///
/// 约定：系统调用返回错误时，用户态看到的是 `-errno`（通常通过 `isize` 表示）。
///
/// English: positive errno values as carried in-kernel; userspace observes `-errno`.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ErrNo(
    /// 正数 errno 数值（与 Linux libc 中 `errno` 含义一致，不含符号位约定）。
    pub isize,
);

impl ErrNo {
    /// 取原始正数 errno（与 libc 中 `errno` 的数值一致）。
    ///
    /// English: returns the positive errno magnitude as stored in-kernel.
    #[inline]
    pub const fn raw(self) -> isize {
        self.0
    }

    /// 转为用户态可见的负返回值（`-errno`）。
    ///
    /// English: maps to the negative value observed from userspace syscalls.
    #[inline]
    pub const fn user_ret(self) -> isize {
        -self.0
    }
}

// Linux errno 常量：与 asm-generic errno 数值一致（riscv64 等架构通用）。
// Linux errno constants: same numeric values as asm-generic (shared across arch ABIs).
impl ErrNo {
    /// 操作不允许。
    pub const EPERM: Self = Self(1);
    /// 无此文件或目录。
    pub const ENOENT: Self = Self(2);
    /// 无此进程。
    pub const ESRCH: Self = Self(3);
    /// 系统调用被中断。
    pub const EINTR: Self = Self(4);
    /// 输入/输出错误。
    pub const EIO: Self = Self(5);
    /// 文件描述符无效。
    pub const EBADF: Self = Self(9);
    /// 无子进程。
    pub const ECHILD: Self = Self(10);
    /// 资源暂时不可用，可重试。
    pub const EAGAIN: Self = Self(11);
    /// 内存不足。
    pub const ENOMEM: Self = Self(12);
    /// 权限不足。
    pub const EACCES: Self = Self(13);
    /// 非法地址。
    pub const EFAULT: Self = Self(14);
    /// 设备或资源忙。
    pub const EBUSY: Self = Self(16);
    /// 文件已存在。
    pub const EEXIST: Self = Self(17);
    /// 参数无效。
    pub const EINVAL: Self = Self(22);
    /// 功能未实现或系统调用号未知。
    pub const ENOSYS: Self = Self(38);
    /// 非目录。
    pub const ENOTDIR: Self = Self(20);
    /// 是目录。
    pub const EISDIR: Self = Self(21);
    /// 文件名过长。
    pub const ENAMETOOLONG: Self = Self(36);
    /// 结果缓冲区过小（如 `getcwd`）。
    pub const ERANGE: Self = Self(34);
    /// 非法 seek。
    pub const ESPIPE: Self = Self(29);
    /// 只读文件系统。
    pub const EROFS: Self = Self(30);
    /// 管道破裂。
    pub const EPIPE: Self = Self(32);
}

/// 内核侧常用的 `Result` 别名：成功载荷为 `T`，失败为 [`ErrNo`]。
///
/// 经转换后才对应用户态可见的负返回值。
///
/// English: kernel `Result` before mapping failures to negative `isize` returns.
pub type KernelResult<T> = core::result::Result<T, ErrNo>;
