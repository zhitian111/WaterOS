use crate::{CpuId, CpuMask};


pub struct Smp {
    pub boot_cpu : CpuId,
    //可用的cpu集合
    pub available_cpus : CpuMask,
    //在线的cpu集合
    pub online_cpus : CpuMask,
    //请求其他cpu发起中断
    pub reschedule : fn(CpuId) -> bool,
}
