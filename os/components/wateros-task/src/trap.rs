//! Trap frame 访问接口：组合层 trap handler 进入/返回时保存与恢复当前任务现场。

/// 保存传入的 frame 到 TCB，并返回保存的 frame 指针。
pub unsafe fn begin_current_trap_frame_access(frame: *mut u8) -> *mut u8 {
    unsafe { crate::runtime::begin_current_trap_frame_access(frame) }
}

/// 将 TCB 中的 frame 写回指针。
pub unsafe fn restore_current_trap_frame(frame: *mut u8) -> bool {
    unsafe { crate::runtime::restore_current_trap_frame(frame) }
}
