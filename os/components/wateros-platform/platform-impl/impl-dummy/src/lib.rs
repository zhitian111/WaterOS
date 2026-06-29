#![no_std]

//! 本模块代码由AI完成
//! **占位**平台实现：启动参数与时间频率 trait 的桩，用于未绑定真实环境的构建。
//!
//! 与 `impl-qemu-riscv64-opensbi` 由 `wateros-platform` feature 切换；方法体多为
//! `unimplemented!()`，不得当作可启动内核的配置。

/// 占位控制台：所有写操作返回 [`PlatformConsoleError::Unsupported`]。
pub mod console {
    use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};

    #[inline]
    pub fn console_flush() -> PlatformConsoleResult<()> { Err(PlatformConsoleError::Unsupported) }

    #[inline]
    pub fn console_write_a_byte(_byte : u8) -> PlatformConsoleResult<()> {
        Err(PlatformConsoleError::Unsupported)
    }

    #[inline]
    pub fn console_write_a_buffer(_bytes : &[u8]) -> PlatformConsoleResult<()> {
        Err(PlatformConsoleError::Unsupported)
    }
}

/// 占位 deadline timer：始终返回 [`PlatformDeadlineTimerError::Unsupported`]。
pub mod timer {
    use api_v0::timer::{
        PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
    };

    #[inline]
    pub fn set_timer(_time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
        Err(PlatformDeadlineTimerError::Unsupported)
    }
}

/// 占位复位：始终返回 [`PlatformResetError::Unsupported`]。
pub mod reset {
    use api_v0::reset::{
        PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
    };

    #[inline]
    pub fn reset(_reset_type : PlatformResetType,
                 _reset_reason : PlatformResetReason)
                 -> PlatformResetResult<()> {
        Err(PlatformResetError::Unsupported)
    }

    #[inline]
    pub fn reboot(reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
        reset(PlatformResetType::ColdReboot,
              reset_reason)
    }

    #[inline]
    pub fn shutdown(reset_reason : PlatformResetReason) -> PlatformResetResult<()> {
        reset(PlatformResetType::Shutdown,
              reset_reason)
    }
}

/// 占位引导参数与上下文。
pub mod boot {
    use api_v0::boot::*;

    /// 占位启动参数（无有效 `a0`/`a1`）。
    #[derive(Debug, Clone, Copy)]
    pub struct PlatformDummyBootArgs;

    /// 占位引导上下文。
    #[derive(Debug, Clone, Copy)]
    pub struct PlatformDummyBootContext;
    impl PlatformBootArgs for PlatformDummyBootArgs {
        #[allow(unused)]
        fn arg0(&self) -> Option<usize> { unimplemented!() }
        #[allow(unused)]
        fn arg1(&self) -> Option<usize> { unimplemented!() }
        #[allow(unused)]
        fn arg2(&self) -> Option<usize> { unimplemented!() }
    }
    impl From<PlatformDummyBootArgs> for PlatformDummyBootContext {
        #[allow(unused)]
        fn from(value : PlatformDummyBootArgs) -> Self { unimplemented!() }
    }
    impl PlatformBootContext<PlatformDummyBootArgs> for PlatformDummyBootContext {}

    pub use PlatformDummyBootArgs as BootArgs;
    pub use PlatformDummyBootContext as BootContext;
}

/// 占位时间频率源。
pub mod time {
    use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    /// 始终返回 [`PlatformTimeError::Unsupported`] 的占位时间源。
    pub struct PlatformDummyTime;

    impl PlatformTime for PlatformDummyTime {
        #[inline]
        fn time_frequency_hz() -> PlatformTimeResult<u64> {
            Err(PlatformTimeError::Unsupported)
        }
    }

    pub use PlatformDummyTime as PlatformTimeImpl;
}
