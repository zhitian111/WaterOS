//! another-ext4 后端注册、探测与 WaterOS 句柄包装。

use super::*;

#[path = "operations.rs"]
mod operations;

pub struct AnotherExt4Impl;
pub static IMPL : AnotherExt4Impl = AnotherExt4Impl;

const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
                                      FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite)];

impl FsImpl for AnotherExt4Impl {
    fn name(&self) -> &'static str { "another-ext4" }
    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }
    fn probe(&self, device : &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        Ok(block_io::probe(device, SUPERBLOCK_MAGIC_OFFSET, EXT4_SUPER_MAGIC)?.then_some(FsKind::Ext4))
    }
    fn mount_ro(&self, device : SharedBlockDevice) -> FsResult<SharedFs> {
        let mut fs = AnotherExt4Fs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalFs::new(Box::new(fs)))))
    }
    fn mount_rw(&self, device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        let mut fs = AnotherExt4Fs::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs)))))
    }
}
