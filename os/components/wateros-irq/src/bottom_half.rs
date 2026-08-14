//! IRQ bottom-half 框架：ISR 只做极简 top-half，heavy 工作延后到可调度上下文。
//!
//! 机制：top-half（trap 上下文，本地中断已屏蔽）把工作写入固定容量环形队列，
//! 并调用已注册的唤醒回调；内核启动一个 bottom-half 任务在 waitqueue 上等待
//! [`has_pending`]，被唤醒后由 [`run_pending`] 逐项执行各 action 的 bottom-half。
//!
//! 锁安全性：环形队列临界区极小（push/pop），ISR 侧本地中断已屏蔽，任务侧
//! pop 后立即释放锁再执行 handler。真实设备 IRQ 启用（T06）前如需进一步消除
//! 任务侧持锁窗口，可在临界区周围掩本地中断。

use spin::Mutex;

use crate::types::Virq;

/// 单条 bottom-half 工作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BottomHalfWork {
    /// 绑定的虚拟中断号。
    pub virq : Virq,
    /// 设备上下文 token（与 action 注册时一致）。
    pub dev_id : usize,
}

/// 固定容量环形队列上限；超限时 [`schedule`] 返回 `false`，调用方必须保证不丢
/// 事件（内核侧在 T06 起把队列扩容或改为按需唤醒）。
const BH_CAPACITY : usize = 64;

fn noop_wake() {}

struct BottomHalfState {
    ring : [Option<BottomHalfWork>; BH_CAPACITY],
    head : usize,
    len : usize,
    wake : fn(),
}

impl BottomHalfState {
    const fn empty() -> Self {
        Self { ring : [const { None }; BH_CAPACITY],
               head : 0,
               len : 0,
               wake : noop_wake }
    }
}

static STATE : Mutex<BottomHalfState> = Mutex::new(BottomHalfState::empty());

/// 注册 ISR 侧唤醒回调：把 bottom-half 内核任务从 waitqueue 唤醒。默认 no-op。
pub fn set_wake_callback(wake : fn()) { STATE.lock().wake = wake; }

/// top-half 侧调度一条 bottom-half 工作；队列满返回 `false`。
pub fn schedule(virq : Virq, dev_id : usize) -> bool {
    let wake = {
        let mut state = STATE.lock();
        if state.len == BH_CAPACITY {
            return false;
        }
        let index = (state.head + state.len) % BH_CAPACITY;
        state.ring[index] = Some(BottomHalfWork { virq, dev_id });
        state.len += 1;
        state.wake
    };
    wake();
    true
}

/// 是否有待处理 bottom-half 工作（bottom-half 任务等待条件）。
pub fn has_pending() -> bool { STATE.lock().len != 0 }

/// 运行全部待处理 bottom-half（由 bottom-half 内核任务调用）。
pub fn run_pending() {
    loop {
        let work = {
            let mut state = STATE.lock();
            if state.len == 0 {
                break;
            }
            let index = state.head;
            state.head = (state.head + 1) % BH_CAPACITY;
            state.len -= 1;
            state.ring[index].take()
        };
        if let Some(work) = work {
            crate::action::run_bottom_half(work.virq, work.dev_id);
        }
    }
}

/// 待处理队列当前长度（诊断用）。
pub fn pending_len() -> usize { STATE.lock().len }

/// 清空待处理队列并复位唤醒回调（仅单元测试使用）。
#[cfg(test)]
pub fn reset_for_test() {
    let mut state = STATE.lock();
    for slot in state.ring
                     .iter_mut()
    {
        *slot = None;
    }
    state.head = 0;
    state.len = 0;
    state.wake = noop_wake;
}
