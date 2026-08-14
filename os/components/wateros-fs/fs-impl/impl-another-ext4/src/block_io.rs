//! Block-device adaptation and backend error handling for another-ext4.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use another_ext4::{Block, BlockDevice, ErrCode, Ext4Error, FileType, BLOCK_SIZE};
use api_v0::{FsError, FsNodeType, FsResult};
use core::sync::atomic::{AtomicBool, Ordering};
use driver_block_api_v0::{Lba, SharedBlockDevice};

pub(crate) fn map_error(error : Ext4Error) -> FsError {
    match error.code() {
        ErrCode::ENOENT => FsError::NotFound,
        ErrCode::EEXIST => FsError::Exists,
        ErrCode::ENOTEMPTY => FsError::NotEmpty,
        ErrCode::ENOTDIR | ErrCode::EISDIR => FsError::NotAFile,
        ErrCode::EINVAL => FsError::InvalidPath,
        ErrCode::ENOSPC => FsError::NoSpace,
        ErrCode::EROFS | ErrCode::ENOTSUP => FsError::Unsupported,
        ErrCode::EIO => FsError::Io,
        _ => FsError::Io,
    }
}

pub(crate) fn map_type(file_type : FileType) -> FsNodeType {
    match file_type {
        FileType::RegularFile => FsNodeType::File,
        FileType::Directory => FsNodeType::Directory,
        FileType::SymLink => FsNodeType::Symlink,
        _ => FsNodeType::Special,
    }
}

/// Adapts WaterOS's block device to another_ext4's fixed-size blocks.
pub(crate) struct BlockAdapter {
    pub(crate) device : SharedBlockDevice,
    pub(crate) io_error : Arc<AtomicBool>,
}

impl BlockDevice for BlockAdapter {
    fn read_block(&self, block_id : u64) -> Block {
        let mut data = Box::new([0u8; BLOCK_SIZE]);
        let mut guard = self.device.lock();
        let block_size = guard.block_size() as u64;
        if block_size == 0 || BLOCK_SIZE as u64 % block_size != 0 {
            log::error!("[fs::another-ext4] unsupported device block size {block_size}, \
                         block={block_id}");
            self.io_error
                .store(true, Ordering::Release);
            return Block::new(block_id, data);
        }
        let lba_count = BLOCK_SIZE as u64 / block_size;
        let result = block_id.checked_mul(lba_count)
                             .ok_or(driver_block_api_v0::DriverError::InvalidParam)
                             .and_then(|lba| {
                                 let end =
                                     lba.checked_add(lba_count)
                                        .ok_or(driver_block_api_v0::DriverError::InvalidParam)?;
                                 if guard.total_blocks()
                                         .is_some_and(|capacity| end > capacity)
                                 {
                                     return Err(driver_block_api_v0::DriverError::InvalidParam);
                                 }
                                 guard.read_blocks(Lba(lba), &mut data[..])
                             });
        if let Err(error) = result {
            self.io_error
                .store(true, Ordering::Release);
            log::error!("[fs::another-ext4] failed to read block {block_id}: {error:?}");
        }
        Block::new(block_id, data)
    }

    fn write_block(&self, block : &Block) {
        let mut guard = self.device.lock();
        let block_size = guard.block_size();
        if block_size == 0 || BLOCK_SIZE % block_size != 0 {
            log::error!("[fs::another-ext4] unsupported device block size {block_size}, block={}",
                        block.id);
            self.io_error
                .store(true, Ordering::Release);
            return;
        }
        let lba_count = BLOCK_SIZE / block_size;
        let lba_count = lba_count as u64;
        let result = block.id
                          .checked_mul(lba_count)
                          .ok_or(driver_block_api_v0::DriverError::InvalidParam)
                          .and_then(|lba| {
                              let end = lba.checked_add(lba_count)
                                           .ok_or(driver_block_api_v0::DriverError::InvalidParam)?;
                              if guard.total_blocks()
                                      .is_some_and(|capacity| end > capacity)
                              {
                                  return Err(driver_block_api_v0::DriverError::InvalidParam);
                              }
                              guard.write_blocks(Lba(lba), &block.data[..])
                          });
        if let Err(error) = result {
            self.io_error
                .store(true, Ordering::Release);
            log::error!("[fs::another-ext4] failed to write block {}: {error:?}",
                        block.id);
        }
    }
}

pub(crate) fn probe(device : &SharedBlockDevice,
                    magic_offset : u64,
                    magic : u16)
                    -> FsResult<bool> {
    let mut bytes = [0u8; 2];
    device.lock()
          .read_bytes(magic_offset, &mut bytes)
          .map_err(|_| FsError::Driver)?;
    Ok(u16::from_le_bytes(bytes) == magic)
}

pub(crate) fn check_backend_error(io_error_state : &Option<Arc<AtomicBool>>) -> FsResult<()> {
    if io_error_state.as_ref()
                     .is_some_and(|state| state.load(Ordering::Acquire))
    {
        return Err(FsError::Io);
    }
    Ok(())
}
