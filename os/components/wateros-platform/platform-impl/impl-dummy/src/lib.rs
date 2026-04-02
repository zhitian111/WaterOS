#![no_std]
pub mod boot {
    use api_v0::boot::*;
    #[derive(Debug, Clone, Copy)]
    pub struct PlatformDummyBootArgs;
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

    pub struct PlatformDummyTime;

    impl PlatformTime for PlatformDummyTime {
        #[inline]
        fn time_frequency_hz() -> PlatformTimeResult<u64> {
            Err(PlatformTimeError::Unsupported)
        }
    }
}
