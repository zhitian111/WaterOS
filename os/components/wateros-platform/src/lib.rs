#![no_std]
#[cfg(feature = "api-v0")]
pub mod boot {
    pub use api_v0::boot::{PlatformBootArgs, PlatformBootContext};
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootArgs as BootArgs;
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootContext as BootContext;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootArgs as BootArgs;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootContext as BootContext;
}
