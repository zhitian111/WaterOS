#![no_std]

pub use api_v0 as api;

#[cfg(feature = "impl-sv39")]
pub use impl_sv39 as mm_impl;

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as mm_impl;

pub fn test_with_range(start_ppn: wateros_base::addr::BasePPN, end_ppn: wateros_base::addr::BasePPN) {
    log::trace!("[wateros-mm] test begin");

    api::test();
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    #[cfg(feature = "impl-sv39")]
    impl_sv39::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[wateros-mm] dummy impl: no mm-impl test");
    }

    log::trace!("[wateros-mm] test end");
}
