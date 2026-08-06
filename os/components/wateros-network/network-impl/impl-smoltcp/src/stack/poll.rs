//! 协议栈轮询及异步状态回收。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Instant;

use super::global::with_stack_if_ready;
use super::state::NetworkStack;
use super::tcp::tcp_is_connected;
use super::types::{SocketConnectError, SocketState};

/// 使用最近一次单调时间驱动协议栈处理一个轮询周期。
///
/// 仅供调用方暂时无法读取平台时钟时兜底；不会把 smoltcp 的时间重置为 0。
pub fn poll() { poll_at_millis(0); }

/// 使用调用方提供的单调毫秒时间驱动协议栈。
pub fn poll_at_millis(millis : i64) { with_stack_if_ready(|stack| stack.poll_at_millis(millis)); }

/// poll 后调用：更新 socket 状态，并回收已完成 TCP 关闭状态机的底层 socket。
pub fn poll_socket_events() { with_stack_if_ready(NetworkStack::poll_socket_events); }

impl NetworkStack {
    fn poll_at_millis(&mut self, millis : i64) {
        let millis = millis.max(self.last_poll_millis);
        self.last_poll_millis = millis;
        let Self { adapter,
                   iface,
                   sockets,
                   .. } = self;
        iface.poll(Instant::from_millis(millis),
                   adapter,
                   sockets);
    }

    fn poll_socket_events(&mut self) {
        // 检查 Connecting → Connected/Closed 转换。RST 或重传耗尽后必须把
        // 失败状态同步到元数据，阻塞 connect 才能退出而不是永久等待。
        let mut updated : BTreeMap<SocketHandle, (SocketState, Option<SocketConnectError>)> =
            BTreeMap::new();
        for (&h, meta) in &self.metas {
            if meta.state == SocketState::Connecting {
                let socket = self.sockets
                                 .get_mut::<tcp::Socket>(h);
                if tcp_is_connected(socket) {
                    // 建连超时只用于 SYN 阶段；成功后必须取消，不能误伤空闲长连接。
                    socket.set_timeout(None);
                    updated.insert(h, (SocketState::Connected, None));
                } else if socket.state() == tcp::State::Closed {
                    let error = if meta.connect_deadline_ms
                                               .is_some_and(|deadline| {
                                                   self.last_poll_millis >= deadline
                                               })
                    {
                        SocketConnectError::TimedOut
                    } else {
                        SocketConnectError::ConnectionRefused
                    };
                    updated.insert(h, (SocketState::Closed, Some(error)));
                }
            }
        }
        for (h, (state, error)) in updated {
            if let Some(meta) = self.metas
                                    .get_mut(&h)
            {
                meta.state = state;
                meta.connection_established = state == SocketState::Connected;
                meta.connect_error = error;
                meta.connect_deadline_ms = None;
            }
        }

        let closed : Vec<SocketHandle> = self.tcp_close_pending
                                             .iter()
                                             .copied()
                                             .filter(|&h| {
                                                 self.sockets
                                                     .get_mut::<tcp::Socket>(h)
                                                     .state() ==
                                                 tcp::State::Closed
                                             })
                                             .collect();
        for handle in closed {
            self.tcp_close_pending
                .remove(&handle);
            self.sockets
                .remove(handle);
        }
    }
}
