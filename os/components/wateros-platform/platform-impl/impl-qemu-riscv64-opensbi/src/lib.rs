#![no_std]
pub mod boot {
    use api_v0::boot::{PlatformBootArgs, PlatformBootContext};

    #[derive(Debug, Clone, Copy)]
    #[allow(unused)]
    pub struct QEMURiscv64OpenSBIBootArgs {
        _arg0: usize,
        _arg1: usize,
    }

    impl PlatformBootArgs for QEMURiscv64OpenSBIBootArgs {
        #[inline]
        fn arg0(&self) -> Option<usize> {
            Some(self._arg0)
        }
        #[inline]
        fn arg1(&self) -> Option<usize> {
            Some(self._arg1)
        }
    }
    impl QEMURiscv64OpenSBIBootArgs {
        #[inline]
        #[allow(unused)]
        pub fn new(arg0: usize, arg1: usize) -> Self {
            Self {
                _arg0: arg0,
                _arg1: arg1,
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    #[allow(unused)]
    pub struct QEMURiscv64OpenSBIBootContext {
        _hart_id: base::cpu::CPUHartID,
        _dtb_pa: base::boot::DTBPA,
    }
    impl From<QEMURiscv64OpenSBIBootArgs> for QEMURiscv64OpenSBIBootContext {
        #[inline]
        #[allow(unused)]
        fn from(value: QEMURiscv64OpenSBIBootArgs) -> Self {
            let hart_id = value.arg0().expect("OpenSBIBoot args error in arg0");
            let dtb_pa = value.arg1().expect("OpenSBIBoot args error in arg1");
            let dtb_pa = base::addr::BasePhysAddr { val: dtb_pa };
            Self {
                _hart_id: hart_id,
                _dtb_pa: dtb_pa,
            }
        }
    }
    impl PlatformBootContext<QEMURiscv64OpenSBIBootArgs> for QEMURiscv64OpenSBIBootContext {}
}
