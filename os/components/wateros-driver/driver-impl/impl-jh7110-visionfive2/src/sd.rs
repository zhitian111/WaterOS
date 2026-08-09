//! Compatibility exports for the shared SD protocol implementation.
//!
//! Hardware activation remains platform-owned and UNVERIFIED until JH7110
//! clocks, reset, pinmux, power and card-detect sequencing are tested on-board.
pub use dw_mmc::sd::*;
