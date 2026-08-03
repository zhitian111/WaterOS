//! 协议栈轮询及异步状态回收。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Instant;

use super::state::{NetworkStack, NETWORK_STACK};
use super::tcp::tcp_is_connected;
use super::types::SocketState;

/// 驱动协议栈处理一个轮询周期：收包 → 分发给 socket → 发送积压包。
///
/// 需要在定时任务中周期性调用。
pub fn poll() { poll_at_millis(0); }

/// 使用调用方提供的单调毫秒时间驱动协议栈。
pub fn poll_at_millis(millis : i64) {
    let mut guard = NETWORK_STACK.lock();
    if let Some(stack) = guard.as_mut() {
        let NetworkStack { adapter,
                           iface,
                           sockets,
                           .. } = stack;
        iface.poll(Instant::from_millis(millis),
                   adapter,
                   sockets);
    }
}

/// poll 后调用：更新 socket 状态，并回收已完成 TCP 关闭状态机的底层 socket。
pub fn poll_socket_events() {
    let mut guard = NETWORK_STACK.lock();
    let stack = match guard.as_mut() {
        Some(s) => s,
        None => return,
    };
    // 检查 Connecting → Connected/Closed 转换。RST 或重传耗尽后必须把
    // 失败状态同步到元数据，阻塞 connect 才能退出而不是永久等待。
    let mut updated : BTreeMap<SocketHandle, SocketState> = BTreeMap::new();
    for (&h, meta) in &stack.metas {
        if meta.state == SocketState::Connecting {
            let socket = stack.sockets
                              .get_mut::<tcp::Socket>(h);
            if tcp_is_connected(socket) {
                updated.insert(h, SocketState::Connected);
            } else if socket.state() == tcp::State::Closed {
                updated.insert(h, SocketState::Closed);
            }
        }
    }
    for (h, state) in updated {
        if let Some(meta) = stack.metas
                                 .get_mut(&h)
        {
            meta.state = state;
        }
    }

    let closed : Vec<SocketHandle> = stack.tcp_close_pending
                                          .iter()
                                          .copied()
                                          .filter(|&h| {
                                              stack.sockets
                                                   .get_mut::<tcp::Socket>(h)
                                                   .state() ==
                                              tcp::State::Closed
                                          })
                                          .collect();
    for handle in closed {
        stack.tcp_close_pending
             .remove(&handle);
        stack.sockets
             .remove(handle);
    }
}
