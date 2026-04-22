#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use spin::Mutex;

pub use driver_api::{DriverError, DriverResult};

pub const BLOCK_SIZE: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lba(pub u64);

impl From<usize> for Lba {
    fn from(value: usize) -> Self { Self(value as u64) }
}

impl From<u64> for Lba {
    fn from(value: u64) -> Self { Self(value) }
}

pub type SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>;

static BLOCK_DEVICES: Mutex<Vec<SharedBlockDevice>> = Mutex::new(Vec::new());

pub trait BlockDevice: Send {
    fn block_size(&self) -> usize { BLOCK_SIZE }

    fn total_blocks(&self) -> Option<u64> { None }

    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()>;

    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()>;

    fn read_bytes(&mut self, offset: u64, dst: &mut [u8]) -> DriverResult<()> {
        if dst.is_empty() {
            return Ok(());
        }

        let block_size = self.block_size();
        if block_size == 0 {
            return Err(DriverError::InvalidParam);
        }

        let start_byte = usize::try_from(offset).map_err(|_| DriverError::InvalidParam)?;
        let end_byte = start_byte
            .checked_add(dst.len())
            .ok_or(DriverError::InvalidParam)?;
        let start_block = start_byte / block_size;
        let end_block = end_byte.div_ceil(block_size);
        let block_count = end_block
            .checked_sub(start_block)
            .ok_or(DriverError::InvalidParam)?;
        let scratch_len = block_count
            .checked_mul(block_size)
            .ok_or(DriverError::InvalidParam)?;
        let mut scratch = vec![0u8; scratch_len];

        self.read_blocks(Lba(start_block as u64), &mut scratch)?;

        let offset_in_block = start_byte % block_size;
        let read_end = offset_in_block
            .checked_add(dst.len())
            .ok_or(DriverError::InvalidParam)?;
        dst.copy_from_slice(&scratch[offset_in_block..read_end]);
        Ok(())
    }

    fn read_prefix(&mut self, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        self.read_bytes(offset, &mut buf)?;
        Ok(buf)
    }
}

pub fn register_block_device(device: SharedBlockDevice) -> usize {
    let mut devices = BLOCK_DEVICES.lock();
    devices.push(device);
    devices.len() - 1
}

pub fn block_device_count() -> usize {
    BLOCK_DEVICES.lock().len()
}

pub fn first_block_device() -> Option<SharedBlockDevice> {
    BLOCK_DEVICES.lock().first().cloned()
}

pub fn test() {
    log::trace!("[driver-block-api] test begin");
    assert_eq!(BLOCK_SIZE, 512);
    let mut sample = SampleBlockDevice::new();
    let prefix = sample.read_prefix(3, 5).expect("prefix read should work");
    assert_eq!(&prefix, &[3, 4, 5, 6, 7]);
    log::trace!("[driver-block-api] test end");
}

struct SampleBlockDevice {
    bytes: [u8; BLOCK_SIZE * 2],
}

impl SampleBlockDevice {
    fn new() -> Self {
        let mut bytes = [0u8; BLOCK_SIZE * 2];
        for (idx, value) in bytes.iter_mut().enumerate() {
            *value = idx as u8;
        }
        Self { bytes }
    }
}

impl BlockDevice for SampleBlockDevice {
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        if !buf.len().is_multiple_of(BLOCK_SIZE) {
            return Err(DriverError::InvalidParam);
        }
        let start = usize::try_from(start_block.0)
            .map_err(|_| DriverError::InvalidParam)?
            .checked_mul(BLOCK_SIZE)
            .ok_or(DriverError::InvalidParam)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(DriverError::InvalidParam)?;
        let src = self.bytes.get(start..end).ok_or(DriverError::InvalidParam)?;
        buf.copy_from_slice(src);
        Ok(())
    }

    fn write_blocks(&mut self, _start_block: Lba, _buf: &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }
}
