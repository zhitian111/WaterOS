//! 阻塞套接字等待：真阻塞 + 可中断（`EINTR`），非阻塞立即 `EAGAIN`。

//! 本模块代码由AI完成
use abi::errno::ErrNo;

/// 阻塞模式下每 tick 检查一次可投递信号；非阻塞模式直接返回 `EAGAIN`。
// 本方法代码由AI完成
pub(crate) fn socket_blocking_tick(nonblocking: bool, task_id: usize) -> Result<(), ErrNo> {
    if nonblocking {
        return Err(ErrNo::EAGAIN);
    }
    if ipc::signal::with_registry(|registry| registry.has_deliverable(task_id).unwrap_or(false)) {
        return Err(ErrNo::EINTR);
    }
    task::sleep_for_ticks(1);
    Ok(())
}
