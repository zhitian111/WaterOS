#![no_std]

//! 单一 ext4 文件系统实现：RO 路径基于 `ext4-view`，RW 路径基于 `ext4plus`，
//! 通过 [`api_v0::FsImpl`] 向聚合层注册一条能力（[`api_v0::FsKind::Ext4`] 同时支持 RO 与 RW）。
//!
//! 后续替换点：RW 路径依赖 `ext4plus` beta 能力，生产级 journal 与崩溃一致性需另行评估。

extern crate alloc;

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
pub use selftest::{ro_self_test, rw_smoke_self_test};

// 与 Linux ext4 superblock 布局一致：卷偏移 1024 起为主 superblock，`s_magic` 固定为 0xEF53。
/// ext4 superblock 中标识 ext2/3/4 的 magic 数。
const EXT4_SUPER_MAGIC: u16 = 0xEF53;
/// superblock 起始字节偏移（位于卷头 1024 字节之后），`s_magic` 在其内偏移 0x38。
const SUPERBLOCK_OFFSET: u64 = 1024;
/// `s_magic` 在 1024 字节 superblock 内的字节偏移。
const MAGIC_OFFSET_IN_SB: usize = 0x38;

/// 通过读取 superblock magic 判定卷是否为 ext2/3/4（轻量探测，不校验完整 checksum）。
fn probe_ext4_magic(device: &SharedBlockDevice) -> FsResult<bool> {
    let mut buf = [0u8; 2];
    let r = device
        .lock()
        .read_bytes(SUPERBLOCK_OFFSET + MAGIC_OFFSET_IN_SB as u64, &mut buf);
    match r {
        Ok(()) => Ok(u16::from_le_bytes(buf) == EXT4_SUPER_MAGIC),
        Err(_) => Err(FsError::Driver),
    }
}

/// 共有 `FsImpl` 入口的具体类型；实例为 [`IMPL`]。
pub struct Ext4FsImpl;

/// ext4 impl 的 ' static 注册项。聚合层应在 `registered_fs_impls()` 中放入 `&IMPL`。
pub static IMPL: Ext4FsImpl = Ext4FsImpl;

const SUPPORTED: &[FsCapability] = &[
    FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
    FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite),
];

impl FsImpl for Ext4FsImpl {
    fn name(&self) -> &'static str { "ext4" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    fn probe(&self, device: &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        if probe_ext4_magic(device)? {
            Ok(Some(FsKind::Ext4))
        } else {
            Ok(None)
        }
    }

    fn mount_ro(&self, device: SharedBlockDevice) -> FsResult<SharedFs> {
        logging::info!("[fs::ext4] mount_ro begin");
        let mut fs = Ext4Fs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        let shared: SharedFs = Arc::new(Mutex::new(LocalFs::new(Box::new(fs))));
        Ok(shared)
    }

    fn mount_rw(&self, device: SharedBlockDevice) -> FsResult<SharedRwFs> {
        logging::info!("[fs::ext4] mount_rw begin");
        let mut fs = Ext4FsRw::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        let shared: SharedRwFs = Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs))));
        Ok(shared)
    }
}
