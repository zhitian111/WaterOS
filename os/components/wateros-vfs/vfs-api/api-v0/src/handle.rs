//! 打开态文件句柄（fd 会话的上游抽象；完整实现见后续工作包）。

extern crate alloc;

use alloc::boxed::Box;

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

    pub const fn read() -> Self {
        Self(Self::READ)
    }

    pub fn contains(&self, flag: u32) -> bool {
        self.0 & flag != 0
    }
}

/// 流式读写的已打开对象（pipe、控制台、文件会话等）。
pub trait VfsIoHandle {
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
        Ok(())
    }

    fn truncate(&mut self, _len: u64) -> VfsResult<()> {
        Err(VfsError::Unsupported)
    }

    /// 若本句柄表示已打开目录，返回其绝对路径（供 `openat(dirfd, …)` 解析相对路径）。
    fn directory_path(&self) -> Option<&str> {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsSeekWhence {
    Set,
    Cur,
    End,
}

/// 路径级打开为句柄。
pub trait VfsOpenOps {
    fn open(&self, path: &str, flags: VfsOpenFlags) -> VfsResult<Box<dyn VfsIoHandle>> {
        let _ = (path, flags);
        Err(VfsError::Unsupported)
    }
}
