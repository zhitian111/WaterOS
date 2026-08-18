//! 堆碎片化压测：模拟 fork/exit 多 size-class 非 LIFO alloc/dealloc churn。

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use config::mm::KERNEL_HEAP_SIZE;

use crate::heap_mem_stats;
use crate::HEAP_ALLOCATOR;

const SIZE_CLASSES : &[usize] = &[64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384];

struct LivePtr {
    /// 当前仍由压测持有的分配地址。
    ptr : NonNull<u8>,
    /// 释放该地址时必须原样传回的布局。
    layout : core::alloc::Layout,
}

/// 多 size-class 随机 alloc/dealloc，周期性打印 `heap_mem_stats` 与批次耗时。
///
/// 不返回：压测结束后打印摘要并 `loop {}` 挂起，便于 QEMU 日志采集。
pub fn heap_fragmentation_stress_report(iterations : usize) -> ! {
    log::warn!("[heap-stress] begin iterations={iterations} cap={KERNEL_HEAP_SIZE}");
    let mut live : [Option<LivePtr>; 256] = core::array::from_fn(|_| None);
    let mut live_count = 0usize;
    let window = iterations / 5;
    let mut early_ns_sum = 0u128;
    let mut early_ns_count = 0u128;
    let mut late_ns_sum = 0u128;
    let mut late_ns_count = 0u128;

    for round in 0..iterations {
        let size = SIZE_CLASSES[round % SIZE_CLASSES.len()];
        let align = 8usize;
        let layout = Layout::from_size_align(size, align).expect("layout");
        let slot = round % live.len();

        let t0 = read_monotonic_raw();
        if let Some(old) = live[slot].take() {
            unsafe {
                GlobalAlloc::dealloc(&HEAP_ALLOCATOR, old.ptr.as_ptr(), old.layout);
            }
            live_count = live_count.saturating_sub(1);
        }
        let ptr = unsafe { GlobalAlloc::alloc(&HEAP_ALLOCATOR, layout) };
        if !ptr.is_null() {
            live[slot] = Some(LivePtr { ptr: unsafe { NonNull::new_unchecked(ptr) },
                                        layout });
            live_count += 1;
        }
        let t1 = read_monotonic_raw();
        let elapsed = t1.saturating_sub(t0);

        if round < window {
            early_ns_sum = early_ns_sum.saturating_add(elapsed);
            early_ns_count += 1;
        } else if round >= iterations - window {
            late_ns_sum = late_ns_sum.saturating_add(elapsed);
            late_ns_count += 1;
        }

        if round > 0 && round % 1000 == 0 {
            let stats = heap_mem_stats();
            log::warn!("[heap-stress] round={round} live={live_count} used={} free={} \
                        batch_raw={elapsed}",
                       stats.used,
                       stats.free);
        }
    }

    for entry in live.iter_mut() {
        if let Some(old) = entry.take() {
            unsafe {
                GlobalAlloc::dealloc(&HEAP_ALLOCATOR, old.ptr.as_ptr(), old.layout);
            }
        }
    }

    let early_avg = if early_ns_count > 0 {
        early_ns_sum / early_ns_count
    } else {
        0
    };
    let late_avg = if late_ns_count > 0 {
        late_ns_sum / late_ns_count
    } else {
        0
    };
    let stats = heap_mem_stats();
    let ratio_pct = if early_avg > 0 {
        (late_avg.saturating_mul(100)) / early_avg
    } else {
        0
    };
    log::warn!("[heap-stress] done used={} free={} early_avg_raw={early_avg} \
                late_avg_raw={late_avg} late/early_pct={ratio_pct}%",
               stats.used,
               stats.free);
    loop {}
}

/// 单调原始计数：优先 RISC-V `time`，否则用迭代序号近似。
fn read_monotonic_raw() -> u128 {
    #[cfg(target_arch = "riscv64")]
    {
        let mut time : u64;
        unsafe {
            core::arch::asm!("csrr {}, time", out(reg) time, options(nomem, nostack));
        }
        return time as u128;
    }
    #[cfg(target_arch = "loongarch64")]
    {
        let mut time : u64;
        unsafe {
            core::arch::asm!("rdtime.d {}, $zero", out(reg) time, options(nomem, nostack));
        }
        return time as u128;
    }
    #[allow(unused)]
    {
        0
    }
}
