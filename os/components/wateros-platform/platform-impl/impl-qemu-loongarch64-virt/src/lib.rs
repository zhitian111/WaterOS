#![no_std]

pub mod boot {
    use api_v0::boot::{PlatformBootArgs, PlatformBootContext};

    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootArgs {
        arg0 : usize,
        arg1 : usize,
        arg2 : usize,
    }

    impl QEMULoongArch64VirtBootArgs {
        #[inline]
        pub const fn new(arg0 : usize, arg1 : usize, arg2 : usize) -> Self {
            Self { arg0, arg1, arg2 }
        }
    }

    impl PlatformBootArgs for QEMULoongArch64VirtBootArgs {
        #[inline]
        fn arg0(&self) -> Option<usize> { Some(self.arg0) }

        #[inline]
        fn arg1(&self) -> Option<usize> { Some(self.arg1) }

        #[inline]
        fn arg2(&self) -> Option<usize> { Some(self.arg2) }
    }

    #[derive(Debug, Clone, Copy)]
    pub struct QEMULoongArch64VirtBootContext {
        pub arg0 : usize,
        pub arg1 : usize,
        pub arg2 : usize,
    }

    impl From<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {
        #[inline]
        fn from(value : QEMULoongArch64VirtBootArgs) -> Self {
            Self { arg0 : value.arg0,
                   arg1 : value.arg1,
                   arg2 : value.arg2 }
        }
    }

    impl PlatformBootContext<QEMULoongArch64VirtBootArgs> for QEMULoongArch64VirtBootContext {}
}

pub mod time {
    use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    pub struct QEMULoongArch64VirtTime;

    impl PlatformTime for QEMULoongArch64VirtTime {
        #[inline]
        fn time_frequency_hz() -> PlatformTimeResult<u64> {
            Err(PlatformTimeError::Unsupported)
        }
    }
}
