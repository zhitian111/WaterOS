//! 全局 klog 实例与中断安全锁入口。

use arch::interrupt::{
    disable_global_interrupt, read_global_interrupt_state, restore_global_interrupt_state,
    ArchInterruptState,
};
use api_v0::{AppendResult, KlogFlags, KlogRecordMeta, KlogStats, KlogStore};
use crate::state::KlogRingbufInner;

/// `LOCK:` 全局环的唯一锁。所有访问都必须先屏蔽本 CPU 全局中断，防止同 CPU 中断日志重入
/// `spin::Mutex` 而自旋死锁。
fn debug_cpu_id() -> usize { arch::cpu::current_cpu_id().raw() }

static KLOG : debug::TrackedMutex<Option<KlogRingbufInner>> =
    debug::TrackedMutex::new(None, debug::DebugLockKind::Klog, debug_cpu_id);

/// 保存并在析构时恢复中断状态；不会无条件开启原本关闭的中断。
struct KlogInterruptGuard {
    state : ArchInterruptState,
}

impl KlogInterruptGuard {
    fn new() -> Self {
        let state = read_global_interrupt_state()
            .expect("read global interrupt state for klog guard");
        disable_global_interrupt().expect("disable global interrupt for klog guard");
        Self { state }
    }
}

impl Drop for KlogInterruptGuard {
    fn drop(&mut self) {
        restore_global_interrupt_state(self.state)
            .expect("restore global interrupt state for klog guard");
    }
}

fn ensure_inner(slot : &mut Option<KlogRingbufInner>) -> &mut KlogRingbufInner {
    if slot.is_none() {
        *slot = Some(KlogRingbufInner::default());
    }
    slot.as_mut().expect("klog inner initialized above")
}

/// 全局内核消息环的零大小访问类型；仅实现 crate 内部使用。
pub(crate) struct KlogRingbuf;

impl KlogRingbuf {
    /// 清空全局环；与普通读写使用相同的 IRQ + mutex 边界。
    pub(crate) fn init() { Self::with(KlogRingbufInner::reset); }

    /// `LOCK:` 在全局锁内访问环状态。
    ///
    /// 闭包不得再次进入 klog、打印日志、调度、访问用户内存或做任何可能长时间阻塞的操作。
    /// 所有需要的外部数据应在调用前准备，副作用应在闭包返回后执行。
    pub(crate) fn with<R>(f : impl FnOnce(&mut KlogRingbufInner) -> R) -> R {
        let _irq = KlogInterruptGuard::new();
        let mut guard = KLOG.lock();
        f(ensure_inner(&mut guard))
    }
}

/// 清空全局 klog 服务。启动阶段可重复调用；普通运行期间不得借此丢弃诊断记录。
pub fn init() { KlogRingbuf::init(); }

/// `FLOW:` 追加一条记录，自动填充单调时间和调用 task ID。
///
/// 这是全局服务的标准写入入口。调用者应先完成可能阻塞的工作；取得 ring 锁后不得再执行
/// 外部回调、调度或用户内存访问。
pub fn record(level : u8, facility : u8, text : &[u8]) -> AppendResult {
    let mut meta = KlogRecordMeta::new(ts_nsec_now(),
                                       0,
                                       facility,
                                       level,
                                       KlogFlags::empty(),
                                       caller_id_now());
    record_with_meta(&mut meta, text)
}

/// 使用调用方填写的记录头追加；实现覆盖 `seq`、`text_len` 和可能的截断 flag。
pub(crate) fn record_with_meta(meta : &mut KlogRecordMeta, text : &[u8]) -> AppendResult {
    KlogRingbuf::with(|ring| ring.append(meta, text))
}

/// 返回锁内复制的全局环统计快照。
pub fn stats() -> KlogStats { KlogRingbuf::with(|ring| ring.stats()) }

/// 单调时钟纳秒；平台时间尚不可用时返回 0，记录仍可安全提交。
pub(crate) fn ts_nsec_now() -> u64 {
    platform::timer::now_duration()
        .map(|duration| {
            duration.as_secs()
                .saturating_mul(1_000_000_000)
                .saturating_add(duration.subsec_nanos() as u64)
        })
        .unwrap_or(0)
}

/// 当前内核任务 ID；早期启动或无调度上下文时返回 0。
pub(crate) fn caller_id_now() -> u32 { task::current_task_id().map(|id| id as u32).unwrap_or(0) }

/// `FLOW:` 对全局消息环执行 `sys_syslog` 操作。
///
/// 用户地址验证与拷贝属于 syscall crate；这里的缓冲区必须已处于内核内存。
pub fn dispatch_kernel(action : i32, kernel_buf : &mut [u8], kernel_len : usize) -> isize {
    crate::syslog::dispatch_kernel(action, kernel_buf, kernel_len)
}
