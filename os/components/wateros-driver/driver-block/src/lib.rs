#![no_std]
pub use api_v0::*;

pub fn test() {
    log::trace!("[driver-block] test begin");
    api_v0::test();
    log::trace!("[driver-block] test end");
}
