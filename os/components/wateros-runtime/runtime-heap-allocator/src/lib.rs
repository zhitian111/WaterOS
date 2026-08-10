#![no_std]
//! 内核全局堆：默认使用 [`rlsf::Tlsf`]（O(1) alloc/dealloc）；可通过
//! feature `impl-linked-list-allocator` 切回 [`linked_list_allocator::LockedHeap`]。
//!
//! 堆大小与对齐来自 `wateros-base-config` 的 MM 配置；[`init`] 必须在任何分配前调用一次。
//!
//! RUNTIME_ORDER: `init` 在 BSP 的单线程启动阶段完成后，AP 才可执行可能分配的路径。
//! ALLOC_SYNC: 后端锁保护分配器元数据，`interrupt_guard` 同时禁止本 CPU 的中断重入。

mod interrupt_guard;
mod stress;
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
mod slab;
#[cfg(all(feature = "impl-tlsf", feature = "tlsf-diagnostics"))]
mod tlsf_diagnostics;

use config::mm::KERNEL_HEAP_SIZE;
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

#[cfg(all(feature = "impl-tlsf", feature = "impl-linked-list-allocator"))]
compile_error!("enable only one of `impl-tlsf` or `impl-linked-list-allocator`");

#[cfg(not(any(feature = "impl-tlsf", feature = "impl-linked-list-allocator")))]
compile_error!("enable `impl-tlsf` (default) or `impl-linked-list-allocator`");

#[cfg(feature = "impl-linked-list-allocator")]
mod backend_linked_list;
#[cfg(feature = "impl-tlsf")]
mod backend_tlsf;

#[cfg(feature = "impl-linked-list-allocator")]
use backend_linked_list as backend;
#[cfg(feature = "impl-tlsf")]
use backend_tlsf as backend;

pub use stress::heap_fragmentation_stress_report;
#[cfg(all(feature = "impl-tlsf", feature = "tlsf-diagnostics"))]
pub use tlsf_diagnostics::{emit_buildstorm_counters, snapshot as tlsf_diagnostics,
                           TlsfDiagnosticClass, TlsfDiagnostics,
                           CLASS_LABELS as TLSF_DIAGNOSTIC_CLASS_LABELS};

/// 内核堆用量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapMemStats {
    /// 已分配字节（实现定义：链表后端为精确值，TLSF 为估算）。
    pub used : usize,
    /// 剩余可用字节。
    pub free : usize,
    /// 堆池总容量（`KERNEL_HEAP_SIZE`）。
    pub capacity : usize,
}

/// Slab ownership split.  `tlsf_owned` is the bytes reserved as slab spans;
/// the other three fields partition usable object bytes within those spans.
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SlabMemStats {
    pub application_live : usize,
    pub per_cpu_cached : usize,
    pub central_free : usize,
    pub tlsf_owned : usize,
}

#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
pub fn slab_mem_stats() -> SlabMemStats {
    let stats = slab::mem_stats();
    SlabMemStats { application_live : stats.application_live,
                   per_cpu_cached : stats.per_cpu_cached,
                   central_free : stats.central_free,
                   tlsf_owned : stats.tlsf_owned }
}

pub(crate) use backend::HEAP_ALLOCATOR;

#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
type DrainRequest = fn(u64) -> bool;
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
static DRAIN_REQUEST : AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
static ONLINE_CPUS : AtomicU64 = AtomicU64::new(0);
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
static DRAIN_EPOCH : AtomicUsize = AtomicUsize::new(0);
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
static DRAIN_ACTIVE : AtomicBool = AtomicBool::new(false);
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
static DRAIN_ACK : [AtomicUsize; config::task::MAX_CPUS] =
    [const { AtomicUsize::new(0) }; config::task::MAX_CPUS];

/// Install the allocation-free platform hook used to deliver slab-drain IPIs.
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
pub fn register_slab_drain_request(handler : DrainRequest) {
    DRAIN_REQUEST.store(handler as usize, Ordering::Release);
}

/// Publish a CPU only after its IPI receive path is ready.
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
pub fn note_allocator_cpu_online(cpu : usize) {
    if cpu < u64::BITS as usize {
        ONLINE_CPUS.fetch_or(1u64 << cpu, Ordering::Release);
    }
}

#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
pub(crate) fn request_remote_slab_drain() {
    if DRAIN_ACTIVE.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }
    let current = arch::cpu::current_cpu_id().raw();
    let targets = ONLINE_CPUS.load(Ordering::Acquire) & !(1u64 << current);
    let raw = DRAIN_REQUEST.load(Ordering::Acquire);
    if targets != 0 && raw != 0 {
        let epoch = DRAIN_EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        let request : DrainRequest = unsafe { core::mem::transmute(raw) };
        if request(targets) {
            let mut pending = targets;
            while pending != 0 {
                let cpu = pending.trailing_zeros() as usize;
                if DRAIN_ACK[cpu].load(Ordering::Acquire) == epoch {
                    pending &= !(1u64 << cpu);
                } else {
                    core::hint::spin_loop();
                }
            }
        }
    }
    DRAIN_ACTIVE.store(false, Ordering::Release);
}

/// IPI receive-side operation; performs no allocation and acknowledges only
/// after the local magazine objects have reached central lists/TLSF.
#[cfg(all(feature = "impl-tlsf", feature = "slab-allocator"))]
pub fn handle_slab_drain_ipi() {
    interrupt_guard::with_allocator_interrupt_guard(|| unsafe {
        backend::drain_local_slabs();
    });
    let cpu = arch::cpu::current_cpu_id().raw();
    if cpu < DRAIN_ACK.len() {
        DRAIN_ACK[cpu].store(DRAIN_EPOCH.load(Ordering::Acquire), Ordering::Release);
    }
}

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

// 128 MiB 堆池单独段 `.kernel.heap`，由链接脚本放在 BSS 末尾，避免堆越界覆盖
// SCHEDULER 等小型内核全局变量（见 platform link.ld）。
#[allow(unused)]
#[link_name = "kernel_heap"]
#[unsafe(link_section = ".kernel.heap")]
pub(crate) static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// 使用静态 `HEAP_SPACE` 初始化堆分配器区域。
///
/// **契约**：仅在单核引导路径、且堆尚未使用时调用；调用方保证无并发重入。
/// 重复初始化会破坏 allocator 元数据，不能作为 AP 初始化步骤调用。
pub fn init() {
    backend::init_heap();
    #[cfg(feature = "stress-on-init")]
    heap_fragmentation_stress_report(100_000);
}
