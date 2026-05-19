//! 打开态文件句柄（fd 会话的上游抽象；完整实现见后续工作包）。

extern crate alloc;

use alloc::boxed::Box;

use crate::error::{VfsError, VfsResult};
use crate::meta::VfsMetadata;

/// `open` 标志位（占位；与 ABI `O_*` 对齐将随 syscall 工作包演进）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VfsOpenFlags(pub u32);

impl VfsOpenFlags {
    pub const READ: u32 = 1;
    pub const WRITE: u32 = 2;
    pub const CREATE: u32 = 4;

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
}

/// 已打开文件的偏移读写（由 [`VfsOpenOps`] 创建）。
pub trait VfsFileHandle: VfsIoHandle {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let _ = (offset, buf);
        Err(VfsError::Unsupported)
    }

    fn write_at(&mut self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let _ = (offset, buf);
        Err(VfsError::Unsupported)
    }

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
