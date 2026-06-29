#![no_std]
//! 本模块代码由AI完成

//! 单一 ext4 文件系统实现：RO 与 RW 路径均基于 `ext4plus`，
//! 通过 [`api_v0::FsImpl`] 向聚合层注册一条能力（[`api_v0::FsKind::Ext4`] 同时支持 RO 与 RW）。
//!
//! 后续替换点：RW 路径依赖 `ext4plus` beta 能力，生产级 journal 与崩溃一致性需另行评估。

extern crate alloc;

mod boot_inspect;
mod ro;
mod rw;
mod selftest;

use alloc::boxed::Box;
use alloc::sync::Arc;
use api_v0::{
    FsAccessMode, FsCapability, FsError, FsImpl, FsKind, FsResult, LocalFs, LocalRwFs, ReadOnlyFs,
    ReadWriteFs, SharedFs, SharedRwFs,
};
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;

pub use ro::Ext4Fs;
pub use rw::Ext4FsRw;
pub use selftest::{ro_self_test, rw_mkdir_verify, rw_self_test};

/// ext4 superblock 中标识 ext2/3/4 的 magic（与 Linux 布局一致：`s_magic` 固定为 0xEF53）。
// 本变量代码由AI完成
const EXT4_SUPER_MAGIC : u16 = 0xEF53;
/// 主 superblock 起始字节偏移（卷头 1024 字节之后）。
// 本变量代码由AI完成
const SUPERBLOCK_OFFSET : u64 = 1024;
/// `s_magic` 在 1024 字节 superblock 内的字节偏移（见内核 `ext4_super_block` 布局）。
// 本变量代码由AI完成
const MAGIC_OFFSET_IN_SB : usize = 0x38;

/// 通过读取 superblock magic 判定卷是否为 ext2/3/4（轻量探测，不校验完整 checksum）。
// 本方法代码由AI完成
fn probe_ext4_magic(device : &SharedBlockDevice) -> FsResult<bool> {
    let mut buf = [0u8; 2];
    let r = device.lock()
                  .read_bytes(SUPERBLOCK_OFFSET + MAGIC_OFFSET_IN_SB as u64,
                              &mut buf);
    match r {
        Ok(()) => Ok(u16::from_le_bytes(buf) == EXT4_SUPER_MAGIC),
        Err(_) => Err(FsError::Driver),
    }
}

/// 共有 `FsImpl` 入口的具体类型；实例为 [`IMPL`]。
// 本结构代码由AI完成
pub struct Ext4FsImpl;

/// ext4 impl 的 ' static 注册项。聚合层应在 `registered_fs_impls()` 中放入 `&IMPL`。
// 本变量代码由AI完成
pub static IMPL : Ext4FsImpl = Ext4FsImpl;

// 本变量代码由AI完成
const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
                                      FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite)];

impl FsImpl for Ext4FsImpl {
    fn name(&self) -> &'static str { "ext4" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

// 本方法代码由AI完成
    fn probe(&self, device : &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        if probe_ext4_magic(device)? {
            Ok(Some(FsKind::Ext4))
        } else {
            Ok(None)
        }
    }

// 本方法代码由AI完成
    fn mount_ro(&self, device : SharedBlockDevice) -> FsResult<SharedFs> {
        logging::info!("[fs::ext4] mount_ro begin");
        let mut fs = Ext4Fs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        let shared : SharedFs = Arc::new(Mutex::new(LocalFs::new(Box::new(fs))));
        Ok(shared)
    }

// 本方法代码由AI完成
    fn mount_rw(&self, device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        logging::trace!("[fs::ext4] mount_rw begin");
        let mut fs = Ext4FsRw::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        let shared : SharedRwFs = Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs))));
        Ok(shared)
    }
}
