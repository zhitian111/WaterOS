//! 硬件线程（hart）标识的类型别名，与 CSR/固件报告中的 hart 编号同一刻度。

/// 逻辑硬件线程（hart）标识；在无 SMT 场景下通常可与物理核一一对应。
pub type CPUHartID = usize;
