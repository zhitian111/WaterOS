//! Block-device adaptation and backend error handling for another-ext4.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use another_ext4::{Block, BlockDevice, ErrCode, Ext4Error, FileType, BLOCK_SIZE};
use api_v0::{FsError, FsNodeType, FsResult};
use driver_block_api_v0::{DriverError, Lba, SharedBlockDevice};
use spin::Mutex;

#[derive(Debug, Clone, Copy)]
pub(crate) enum BlockOperation {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockIoFailure {
    operation: BlockOperation,
    fs_block: u64,
    lba: Option<u64>,
    sectors: u64,
    capacity: Option<u64>,
    device_block_size: u64,
    error: DriverError,
}

pub(crate) type BackendErrorState = Arc<Mutex<Option<BlockIoFailure>>>;

fn map_driver_error(error: DriverError) -> FsError {
    match error {
        DriverError::Unsupported => FsError::Unsupported,
        DriverError::InvalidParam | DriverError::OutOfRange => FsError::Corrupt,
        DriverError::InvalidDtb |
        DriverError::NotFound |
        DriverError::NotReady |
        DriverError::NoMemory |
        DriverError::Protocol |
        DriverError::IoError => FsError::Driver,
    }
}

pub(crate) fn map_error(error: Ext4Error) -> FsError {
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

pub(crate) fn map_type(file_type: FileType) -> FsNodeType {
    match file_type {
        FileType::RegularFile => FsNodeType::File,
        FileType::Directory => FsNodeType::Directory,
        FileType::SymLink => FsNodeType::Symlink,
        _ => FsNodeType::Special,
    }
}

/// Adapts WaterOS's block device to another_ext4's fixed-size blocks.
pub(crate) struct BlockAdapter {
    pub(crate) device: SharedBlockDevice,
    pub(crate) io_error: BackendErrorState,
}

impl BlockAdapter {
    fn record_failure(&self, failure: BlockIoFailure) {
        let mut first = self.io_error.lock();
        if first.is_some() {
            return;
        }
        log::error!("[fs::another-ext4] backend failure op={:?} fs_block={} lba={:?} sectors={} capacity={:?} device_block_size={} error={:?}",
                    failure.operation,
                    failure.fs_block,
                    failure.lba,
                    failure.sectors,
                    failure.capacity,
                    failure.device_block_size,
                    failure.error);
        *first = Some(failure);
    }

    fn geometry(&self,
                operation: BlockOperation,
                fs_block: u64,
                block_size: u64,
                capacity: Option<u64>)
                -> Option<(u64, u64)> {
        if block_size == 0 || BLOCK_SIZE as u64 % block_size != 0 {
            self.record_failure(BlockIoFailure { operation,
                                                 fs_block,
                                                 lba: None,
                                                 sectors: 0,
                                                 capacity,
                                                 device_block_size: block_size,
                                                 error: DriverError::Unsupported });
            return None;
        }
        let sectors = BLOCK_SIZE as u64 / block_size;
        let Some(lba) = fs_block.checked_mul(sectors) else {
            self.record_failure(BlockIoFailure { operation,
                                                 fs_block,
                                                 lba: None,
                                                 sectors,
                                                 capacity,
                                                 device_block_size: block_size,
                                                 error: DriverError::OutOfRange });
            return None;
        };
        let in_range = lba.checked_add(sectors)
                          .is_some_and(|end| capacity.is_none_or(|total| end <= total));
        if !in_range {
            self.record_failure(BlockIoFailure { operation,
                                                 fs_block,
                                                 lba: Some(lba),
                                                 sectors,
                                                 capacity,
                                                 device_block_size: block_size,
                                                 error: DriverError::OutOfRange });
            return None;
        }
        Some((lba, sectors))
    }
}

impl BlockDevice for BlockAdapter {
    fn read_block(&self, block_id: u64) -> Block {
        let mut data = Box::new([0u8; BLOCK_SIZE]);
        let mut guard = self.device.lock();
        let block_size = guard.block_size() as u64;
        let capacity = guard.total_blocks();
        let Some((lba, sectors)) = self.geometry(BlockOperation::Read,
                                                 block_id,
                                                 block_size,
                                                 capacity) else {
            return Block::new(block_id, data);
        };
        let result = guard.read_blocks(Lba(lba), &mut data[..]);
        drop(guard);
        if let Err(error) = result {
            self.record_failure(BlockIoFailure { operation: BlockOperation::Read,
                                                 fs_block: block_id,
                                                 lba: Some(lba),
                                                 sectors,
                                                 capacity,
                                                 device_block_size: block_size,
                                                 error });
        }
        Block::new(block_id, data)
    }

    fn write_block(&self, block: &Block) {
        let mut guard = self.device.lock();
        let block_size = guard.block_size() as u64;
        let capacity = guard.total_blocks();
        let Some((lba, sectors)) = self.geometry(BlockOperation::Write,
                                                 block.id,
                                                 block_size,
                                                 capacity) else {
            return;
        };
        let result = guard.write_blocks(Lba(lba), &block.data[..]);
        drop(guard);
        if let Err(error) = result {
            self.record_failure(BlockIoFailure { operation: BlockOperation::Write,
                                                 fs_block: block.id,
                                                 lba: Some(lba),
                                                 sectors,
                                                 capacity,
                                                 device_block_size: block_size,
                                                 error });
        }
    }
}

pub(crate) fn probe(device: &SharedBlockDevice, magic_offset: u64, magic: u16) -> FsResult<bool> {
    let mut bytes = [0u8; 2];
    device.lock().read_bytes(magic_offset, &mut bytes).map_err(map_driver_error)?;
    Ok(u16::from_le_bytes(bytes) == magic)
}

pub(crate) fn check_backend_error(io_error_state: &Option<BackendErrorState>) -> FsResult<()> {
    if let Some(error) = io_error_state.as_ref().and_then(|state| *state.lock()) {
        return Err(map_driver_error(error.error));
    }
    Ok(())
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    let state = Some(Arc::new(Mutex::new(None)));
    assert_eq!(check_backend_error(&state), Ok(()));
    *state.as_ref().unwrap().lock() = Some(BlockIoFailure {
        operation: BlockOperation::Read,
        fs_block: u64::MAX,
        lba: None,
        sectors: 8,
        capacity: Some(1024),
        device_block_size: 512,
        error: DriverError::OutOfRange,
    });
    assert_eq!(check_backend_error(&state), Err(FsError::Corrupt));
}
