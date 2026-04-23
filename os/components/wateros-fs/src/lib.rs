#![no_std]
extern crate alloc;

pub mod api {
    pub use ::api_v0::*;
}
pub mod devfs {
    pub use ::devfs::*;
}
pub mod rootfs {
    pub use ::rootfs::*;
}

#[cfg(feature = "impl-ext4-view")]
pub use impl_ext4_view as active_impl;
#[cfg(all(not(feature = "impl-ext4-view"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

pub use api_v0::*;
pub use devfs::*;
pub use rootfs::*;

pub fn init() {
    #[cfg(feature = "impl-ext4-view")]
    {
        logging::info!("[fs] init begin");
        let node_count = devfs::active_impl::refresh();
        logging::info!("[fs] devfs refreshed, nodes={}", node_count);
        match rootfs::active_impl::mount_default_root() {
            Ok(()) => logging::info!("[fs] root fs mounted"),
            Err(err) => logging::warn!("[fs] init failed: {:?}", err),
        }
    }
}

pub fn test() {
    logging::trace!("[fs] test begin");
    api_v0::test();
    #[cfg(feature = "impl-ext4-view")]
    {
        let Some(fs) = rootfs::active_impl::root_fs() else {
            logging::warn!("[fs] ext4-view test skipped: {:?}", api_v0::FsError::NotMounted);
            logging::trace!("[fs] test end");
            return;
        };
        if let Err(err) = impl_ext4_view::test_with(fs) {
            logging::warn!("[fs] ext4-view test failed: {:?}", err);
        }
    }
    logging::trace!("[fs] test end");
}
