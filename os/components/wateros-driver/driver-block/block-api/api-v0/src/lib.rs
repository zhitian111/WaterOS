#![no_std]

pub use driver_api::{DriverError, DriverResult};

pub const BLOCK_SIZE: usize = 512;

pub trait BlockDevice {
    fn read_blocks(&mut self, start_block: usize, buf: &mut [u8]) -> DriverResult<()>;
    fn write_blocks(&mut self, start_block: usize, buf: &[u8]) -> DriverResult<()>;
}

pub fn test() {
    log::trace!("[driver-block-api] test begin");
    assert_eq!(BLOCK_SIZE, 512);
    log::trace!("[driver-block-api] test end");
}
