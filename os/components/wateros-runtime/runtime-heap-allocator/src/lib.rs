#![no_std]
//! 内核全局堆：默认使用 [`rlsf::Tlsf`]（O(1) alloc/dealloc）；也可选择
//! `impl-per-cpu-tlsf` 固定 arena 后端或 `impl-linked-list-allocator` 回退后端。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。
//!
//! RUNTIME_ORDER: `init` 在 BSP 的单线程启动阶段完成后，AP 才可执行可能分配的路径。
//! ALLOC_SYNC: 后端锁保护分配器元数据，`interrupt_guard` 同时禁止本 CPU 的中断重入。

extern crate alloc;

mod interrupt_guard;
mod stress;

use config::mm::KERNEL_HEAP_SIZE;
#[cfg(feature = "impl-per-cpu-tlsf")]
use config::task::MAX_CPUS;

#[cfg(any(all(feature = "impl-tlsf", feature = "impl-linked-list-allocator"),
          all(feature = "impl-tlsf", feature = "impl-per-cpu-tlsf"),
          all(feature = "impl-linked-list-allocator", feature = "impl-per-cpu-tlsf")))]
compile_error!("enable exactly one heap backend");

#[cfg(not(any(feature = "impl-tlsf",
              feature = "impl-per-cpu-tlsf",
              feature = "impl-linked-list-allocator")))]
compile_error!("enable `impl-tlsf` (default), `impl-per-cpu-tlsf`, or `impl-linked-list-allocator`");

#[cfg(feature = "impl-linked-list-allocator")]
mod backend_linked_list;
#[cfg(feature = "impl-tlsf")]
mod backend_tlsf;
#[cfg(feature = "impl-per-cpu-tlsf")]
mod backend_per_cpu_tlsf;

#[cfg(feature = "impl-linked-list-allocator")]
use backend_linked_list as backend;
#[cfg(feature = "impl-tlsf")]
use backend_tlsf as backend;
#[cfg(feature = "impl-per-cpu-tlsf")]
use backend_per_cpu_tlsf as backend;

pub use stress::heap_fragmentation_stress_report;

/// 内核堆用量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapMemStats {
    /// 已分配字节（实现定义：链表后端为精确值，TLSF 为估算）。
    pub used : usize,
    /// 剩余可用字节。
    pub free : usize,
    /// 后端管理的堆池总容量；per-CPU 后端包含 global 与全部 CPU arena。
    pub capacity : usize,
}

pub(crate) use backend::HEAP_ALLOCATOR;

/// 返回当前内核堆用量（`used`/`free`/`capacity`）。
///
/// 这是诊断快照：拿到值后 allocator 可立即变化；TLSF backend 的 `used` 还是按 layout
/// 大小累计的估算值，不能用于内存回收决策。
pub fn heap_mem_stats() -> HeapMemStats {
    interrupt_guard::with_allocator_interrupt_guard(|| backend::stats())
}

/// 堆分配失败路径：由内核 `#[alloc_error_handler]` 委托（见 `wateros` 根 crate），打印布局后 panic。
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    let stats = heap_mem_stats();
    log::warn!("[heap] OOM: layout_size={} align={} used={} free={} cap={}",
               layout.size(),
               layout.align(),
               stats.used,
               stats.free,
               stats.capacity);
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

// 堆池单独放入 `.kernel.heap`，由链接脚本放在 BSS 末尾，避免堆越界覆盖
// SCHEDULER 等小型内核全局变量（见 platform link.ld）。
#[allow(unused)]
#[link_name = "kernel_heap"]
#[unsafe(link_section = ".kernel.heap.global")]
pub(crate) static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// per-CPU TLSF 后端的固定 CPU arena；每个元素的容量都是 `KERNEL_HEAP_SIZE`。
#[cfg(feature = "impl-per-cpu-tlsf")]
#[allow(unused)]
#[unsafe(link_section = ".kernel.heap.percpu")]
pub(crate) static mut PER_CPU_HEAP_SPACE : [[u8; KERNEL_HEAP_SIZE]; MAX_CPUS] =
    [[0; KERNEL_HEAP_SIZE]; MAX_CPUS];

/// 返回链接镜像中为 heap arena 保留的最高地址（半开区间末端）。
///
/// 通过近端静态池起始地址加编译期容量计算，避免巨型 per-CPU 段末端超出架构
/// PC-relative relocation 范围。
pub fn reserved_end() -> usize { backend::reserved_end() }

/// 使用静态 `HEAP_SPACE` 初始化堆分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；调用方保证无并发重入。
/// 重复初始化会破坏 allocator 元数据，不能作为 AP 初始化步骤调用。
pub fn init() {
    backend::init_heap();
    #[cfg(feature = "stress-on-init")]
    heap_fragmentation_stress_report(100_000);
}

/// 允许新分配从早期 global arena 切换到当前 CPU 的固定 arena。
///
/// 普通单堆后端中这是空操作。per-CPU 后端要求 BSP 在核心子系统初始化完成、发布 AP
/// 启动条件之前调用一次；切换前产生的对象仍可按地址归还 global arena。
pub fn enable_per_cpu_arenas() { backend::enable_per_cpu_arenas(); }

#[cfg(feature = "self_test")]
/// 堆组件可用性自检：申请、写入、校验并释放临时分配。
pub fn self_test() {
    use alloc::boxed::Box;
    log::info!("[heap] self_test begin");
    let mut value = Box::new([0u8; 128]);
    value[0] = 0x5a;
    value[127] = 0xa5;
    assert_eq!(value[0], 0x5a);
    assert_eq!(value[127], 0xa5);
    drop(value);
    log::info!("[heap] self_test complete; allocation released");
}
