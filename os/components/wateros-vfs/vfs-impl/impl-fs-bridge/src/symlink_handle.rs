//! `openat(O_PATH|O_NOFOLLOW)` 打开符号链接节点本身得到的只读句柄。
//!
//! Linux 允许以 `O_PATH|O_NOFOLLOW` 打开符号链接并获得指向链接自身的 fd；
//! systemd-tmpfiles 依赖该行为：先 `symlinkat()`（已存在则 EEXIST），再打开
//! 链接本身 `fstat`/`readlinkat(fd, "")` 校验目标，并通过
//! `fchownat(fd, "", …, AT_EMPTY_PATH)` 设置属主。本句柄只承载路径与元数据，
//! 不做数据 I/O。
// 本模块代码由AI完成
use alloc::{boxed::Box, string::String};

use api_v0::{VfsError, VfsIoHandle, VfsMetadata, VfsResult};

/// 符号链接节点的路径型句柄（仅 `O_PATH` 打开产生）。
#[derive(Clone)]
// 本结构代码由AI完成
pub struct SymlinkPathHandle {
    abs : String,
    meta : VfsMetadata,
}

impl SymlinkPathHandle {
    pub fn new(abs : String, meta : VfsMetadata) -> Self { Self { abs, meta } }
}

impl VfsIoHandle for SymlinkPathHandle {
    fn open_accmode(&self) -> u32 { 0 }

    // 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> { Ok(self.meta.clone()) }

    // 本方法代码由AI完成
    fn backing_path(&self) -> Option<&str> { Some(self.abs.as_str()) }

    // 本方法代码由AI完成
    fn read(&mut self, _buf : &mut [u8]) -> VfsResult<usize> { Err(VfsError::BadFd) }

    // 本方法代码由AI完成
    fn write(&mut self, _buf : &[u8]) -> VfsResult<usize> { Err(VfsError::BadFd) }

    // 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> { Ok(Box::new(self.clone())) }
}
