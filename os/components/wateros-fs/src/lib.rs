#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-ext4-view")]
pub use impl_ext4_view as active_impl;
#[cfg(all(not(feature = "impl-ext4-view"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

pub use api_v0::*;

pub fn init() {
    #[cfg(feature = "impl-ext4-view")]
    {
        if let Err(err) = impl_ext4_view::init() {
            log::warn!("[fs] init failed: {:?}", err);
        }
    }
}

pub fn test() {
    log::trace!("[fs] test begin");
    api_v0::test();
    #[cfg(feature = "impl-ext4-view")]
    {
        if let Err(err) = impl_ext4_view::test() {
            log::warn!("[fs] ext4-view test failed: {:?}", err);
        }
    }
    log::trace!("[fs] test end");
}
