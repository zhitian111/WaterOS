#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub mod waitqueue {
    pub use ::waitqueue::*;
}
