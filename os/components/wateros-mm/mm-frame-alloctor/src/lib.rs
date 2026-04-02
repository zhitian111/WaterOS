#![no_std]

pub use api_v0::*;

#[cfg(feature = "impl-stack")]
pub use impl_stack::*;

#[cfg(feature = "impl-dummy")]
pub use impl_dummy::*;

pub fn test_with_range(start_ppn: wateros_base::addr::BasePPN, end_ppn: wateros_base::addr::BasePPN) {
    log::trace!("[frame-alloctor] test begin");
    #[cfg(feature = "impl-stack")]
    impl_stack::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[frame-alloctor] dummy impl: no test");
    }
    log::trace!("[frame-alloctor] test end");
}
