//! 打开态文件句柄（fd 会话的上游抽象；完整实现见后续工作包）。

extern crate alloc;

use alloc::boxed::Box;
use core::any::Any;

use crate::error::{VfsError, VfsResult};
use crate::meta::VfsMetadata;

/// `open` 标志位（占位；与 ABI `O_*` 对齐将随 syscall 工作包演进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VfsOpenFlags(pub u32);

impl VfsOpenFlags {
    /// 语义层读（与 Linux `O_RDONLY` 对应，Linux 值为 0）。
    pub const READ: u32 = 1;
    /// 语义层写（对应 `O_WRONLY` / `O_RDWR` 的写侧）。
    pub const WRITE: u32 = 2;
    /// 不存在则创建（`O_CREAT`）。
    pub const CREATE: u32 = 4;
    /// 打开时截断（`O_TRUNC`）。
    pub const TRUNC: u32 = 8;
    /// 追加写（`O_APPEND`）。
    pub const APPEND: u32 = 16;
    /// 仅打开目录（`O_DIRECTORY`）。
    pub const DIRECTORY: u32 = 32;

    /// 只读打开标志组合。
    pub const fn read() -> Self {
        Self(Self::READ)
    }

    /// 是否包含指定标志位。
    pub fn contains(&self, flag: u32) -> bool {
        self.0 & flag != 0
    }
}

/// 为 fd 句柄提供对象安全的类型识别，供 syscall 层从统一 VFS fd 表中
/// 识别 socket、epoll 等特殊句柄。
pub trait VfsHandleAny {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Any> VfsHandleAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 流式读写的已打开对象（pipe、控制台、文件会话等）。
pub trait VfsIoHandle: Send + VfsHandleAny {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    fn close(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::Unsupported)
    }

    fn seek(&mut self, _offset: i64, _whence: VfsSeekWhence) -> VfsResult<u64> {
        Err(VfsError::Unsupported)
    }

    /// 在绝对文件偏移处读（`pread` 族）；默认不支持（pipe/socket 等）。
    /// 不得修改顺序 `read` 使用的当前偏移。
    fn read_at(&mut self, _offset: u64, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }

    /// 在绝对文件偏移处写（`pwrite` 族）；默认不支持。
    fn write_at(&mut self, _offset: u64, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }

    fn flush(&mut self) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }

    fn truncate(&mut self, _len: u64) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }

    /// 若本句柄表示已打开目录，返回其绝对路径（供 `openat(dirfd, …)` 解析相对路径）。
    fn directory_path(&self) -> Option<&str> {
        None
    }

    /// 若本句柄对应路径型文件/目录，返回其绝对路径（供 `f*xattr` 等使用）。
    fn backing_path(&self) -> Option<&str> {
        None
    }

    /// `flock(2)` 的打开文件描述 owner；`dup`/`fork` 复制时应保持不变。
    fn flock_owner_id(&self) -> Option<u64> {
        None
    }

    /// 将目录项写入 `buf`（`getdents64` 布局）；非目录句柄默认 [`VfsError::Unsupported`]。
    fn fill_getdents64(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let _ = buf;
        Err(VfsError::Unsupported)
    }

    /// 复制本句柄为新的独立 fd 对象（`dup` / `fork` 继承）。
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Err(VfsError::Unsupported)
    }

    fn ioctl(&mut self, _request: usize, _arg: usize) -> VfsResult<isize> {
        Err(VfsError::Unsupported)
    }

    /// 是否为软件 RTC 字符设备（syscall 层对 rtc fd 分发专用 ioctl）。
    fn is_rtc_device(&self) -> bool {
        false
    }

    /// 是否为 TTY 类字符设备（UART/console 等；syscall 层分发 TTY ioctl）。
    fn is_tty_char_device(&self) -> bool {
        false
    }

    /// `poll`/`ppoll` 就绪位查询；默认不支持（`POLLNVAL` 由 syscall 层处理）。
    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let _ = events;
        Err(VfsError::Unsupported)
    }

    /// Linux `fcntl(F_GETFL)` 状态位（不含 `O_ACCMODE`）；默认无 `O_NONBLOCK`。
    fn open_status_flags(&self) -> u32 {
        0
    }

    /// 在不执行 I/O 的情况下验证句柄是否允许 `read(2)`。
    fn validate_read_access(&self) -> VfsResult<()> {
        const O_ACCMODE: u32 = 3;
        const O_WRONLY: u32 = 1;
        if self.open_accmode() & O_ACCMODE == O_WRONLY {
            Err(VfsError::BadFd)
        } else {
            Ok(())
        }
    }

    /// Linux `fcntl(F_GETFL)` 访问模式（`O_ACCMODE`：0/1/2）。
    ///
    /// 访问模式没有安全的默认值；每类句柄必须明确声明。
    fn open_accmode(&self) -> u32;

    /// Linux `fcntl(F_SETFL)`；默认忽略（pipe/socket 等由具体实现覆盖）。
    fn set_open_status_flags(&mut self, flags: u32) -> VfsResult<()> {
        let _ = flags;
        Ok(())
    }

    /// 在句柄对应 waitqueue 上阻塞；`still_waiting` 为假时返回（多 fd poll 重扫）。
    fn poll_wait_for_ticks(
        &mut self,
        events: i16,
        timeout_ticks: u64,
        still_waiting: &mut dyn FnMut() -> bool,
    ) -> VfsResult<()> {
        let _ = (events, timeout_ticks, still_waiting);
        Err(VfsError::Unsupported)
    }

    /// pipe fd：`F_GETPIPE_SZ` 返回容量；非 pipe 返回 `None`。
    fn pipe_capacity(&self) -> Option<usize> {
        None
    }

    /// pipe fd：返回当前已缓冲字节数；非 pipe 返回 `None`。
    fn pipe_buffer_len(&self) -> Option<usize> {
        None
    }

    /// pipe fd：`F_SETPIPE_SZ` 调整容量；非 pipe 返回 [`VfsError::Unsupported`]。
    fn pipe_set_capacity(&mut self, _capacity: usize) -> VfsResult<usize> {
        Err(VfsError::Unsupported)
    }
}

/// 已打开文件的偏移读写（由 [`VfsOpenOps`] 创建）；`read_at`/`write_at` 见 [`VfsIoHandle`]。
pub trait VfsFileHandle: VfsIoHandle {
    fn seek(&mut self, offset: i64, whence: VfsSeekWhence) -> VfsResult<u64> {
        let _ = (offset, whence);
        Err(VfsError::Unsupported)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Err(VfsError::Unsupported)
    }
}

/// `lseek` / `seek` 基准：绝对偏移、相对当前、相对 EOF。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsSeekWhence {
    /// `SEEK_SET`：相对文件开头。
    Set,
    /// `SEEK_CUR`：相对当前偏移。
    Cur,
    /// `SEEK_END`：相对文件末尾。
    End,
}

/// 路径级打开为句柄。
pub trait VfsOpenOps {
    /// 按绝对路径与标志打开；返回的对象经 fd 会话分配编号后供 syscall 使用。
    fn open(&self, path: &str, flags: VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        let _ = (path, flags);
        Err(VfsError::Unsupported)
    }
}
