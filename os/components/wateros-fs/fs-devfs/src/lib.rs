#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;
#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;
#[cfg(all(not(feature = "impl-kernel"), feature = "impl-dummy"))]
pub use impl_dummy as active_impl;

