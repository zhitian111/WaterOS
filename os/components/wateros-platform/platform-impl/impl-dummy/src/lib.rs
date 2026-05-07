#![no_std]

//! **占位**平台实现：启动参数与时间频率 trait 的桩，用于未绑定真实环境的构建。
//!
//! 与 `impl-qemu-riscv64-opensbi` 由 `wateros-platform` feature 切换；方法体多为
//! `unimplemented!()`，不得当作可启动内核的配置。

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
}

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
}
