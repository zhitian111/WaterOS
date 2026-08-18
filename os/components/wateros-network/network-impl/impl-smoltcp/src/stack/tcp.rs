//! TCP socket 创建、监听与 accept 槽池管理。

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;
use smoltcp::wire::IpAddress;

use super::global::with_stack_mut;
use super::socket::listen_endpoint;
use super::state::{
    NetworkStack, SocketMeta, TcpListenerGroup, TCP_LISTEN_BACKLOG_MAX, TCP_RX_BUFFER_SIZE,
    TCP_TX_BUFFER_SIZE,
};
use super::types::{NetworkError, SocketKind, SocketState};

#[derive(Clone, Copy)]
struct TcpListenerSlotConfig {
    /// 监听槽所属的逻辑 listener 组。
    group_id : u64,
    /// 绑定地址；`None` 表示监听本机任意地址。
    local_ip : Option<[u8; 4]>,
    /// 监听端口，必须是已分配的非零端口。
    port : u16,
    /// 接收超时，单位为毫秒；`None` 表示不设置超时。
    recv_timeout_ms : Option<u64>,
    /// 是否为该连接启用 TCP_NODELAY。
    tcp_nodelay : bool,
    /// 发送缓冲区大小，单位为字节。
    snd_buf_size : i32,
    /// 接收缓冲区大小，单位为字节。
    rcv_buf_size : i32,
}

pub(super) fn tcp_listener_slot_count(backlog : usize) -> usize {
    // Linux 将 backlog 定义为已建立但仍等待 accept() 的连接队列，
    // 当前正在交给 accept() 的连接不计入该队列。除请求深度外保留一个过渡槽，
    // 使用户态服务器处理已接受连接期间仍能创建替代监听器。
    backlog.max(1)
           .saturating_add(1)
           .min(TCP_LISTEN_BACKLOG_MAX)
}

/// TCP 三次握手已经完成，连接至少曾进入可传输数据的状态。
pub(super) fn tcp_is_connected(socket : &tcp::Socket) -> bool {
    matches!(socket.state(),
             tcp::State::Established |
             tcp::State::FinWait1 |
             tcp::State::FinWait2 |
             tcp::State::CloseWait)
}

/// 监听 socket 只有完成握手后才可被 accept；`SynReceived` 还不能交给用户态。
pub(super) fn tcp_is_accept_ready(socket : &tcp::Socket) -> bool {
    matches!(socket.state(),
             tcp::State::Established | tcp::State::CloseWait)
}

fn new_tcp_socket() -> tcp::Socket<'static> {
    let rx = tcp::SocketBuffer::new(vec![0; TCP_RX_BUFFER_SIZE]);
    let tx = tcp::SocketBuffer::new(vec![0; TCP_TX_BUFFER_SIZE]);
    tcp::Socket::new(rx, tx)
}

fn new_tcp_listener_socket(local_ip : Option<[u8; 4]>,
                           port : u16,
                           tcp_nodelay : bool)
                           -> Result<tcp::Socket<'static>, NetworkError> {
    let mut socket = new_tcp_socket();
    socket.set_nagle_enabled(!tcp_nodelay);
    socket.set_ack_delay(if tcp_nodelay {
                             None
                         } else {
                             Some(Duration::from_millis(10))
                         });
    socket.listen(listen_endpoint(local_ip, port))
          .map_err(|_| NetworkError::AddressInUse)?;
    Ok(socket)
}

impl NetworkStack {
    fn create_tcp_socket(&mut self) -> Result<SocketHandle, NetworkError> {
        let handle = self.sockets
                         .add(new_tcp_socket());
        self.metas
            .insert(handle, SocketMeta::new(SocketKind::Tcp));
        Ok(handle)
    }

    fn register_tcp_listener_slot(&mut self,
                                  socket : tcp::Socket<'static>,
                                  config : TcpListenerSlotConfig)
                                  -> SocketHandle {
        let handle = self.sockets
                         .add(socket);
        let mut meta = SocketMeta::new(SocketKind::Tcp);
        meta.state = SocketState::Listening { port : config.port };
        meta.local_ip = config.local_ip;
        meta.local_port = config.port;
        meta.is_listener = true;
        meta.listener_group = Some(config.group_id);
        meta.recv_timeout_ms = config.recv_timeout_ms;
        meta.tcp_nodelay = config.tcp_nodelay;
        meta.snd_buf_size = config.snd_buf_size;
        meta.rcv_buf_size = config.rcv_buf_size;
        self.metas
            .insert(handle, meta);
        handle
    }

    fn add_tcp_listener_slot(&mut self,
                             config : TcpListenerSlotConfig)
                             -> Result<SocketHandle, NetworkError> {
        let socket = new_tcp_listener_socket(config.local_ip,
                                             config.port,
                                             config.tcp_nodelay)?;
        Ok(self.register_tcp_listener_slot(socket, config))
    }

    fn listen(&mut self, handle : SocketHandle, backlog : usize) -> Result<(), NetworkError> {
        let (mut port, local_ip, recv_timeout_ms, tcp_nodelay, snd_buf_size, rcv_buf_size) = {
            let meta = self.socket_meta(handle)?;
            if meta.kind != SocketKind::Tcp {
                return Err(NetworkError::WrongSocketType);
            }
            let port = match meta.state {
                SocketState::Bound { port } => port,
                _ => return Err(NetworkError::NotBound),
            };
            (port,
             meta.local_ip,
             meta.recv_timeout_ms,
             meta.tcp_nodelay,
             meta.snd_buf_size,
             meta.rcv_buf_size)
        };
        if port == 0 {
            port = self.next_ephemeral_port();
            let meta = self.socket_meta_mut(handle)?;
            meta.state = SocketState::Bound { port };
            meta.local_port = port;
        }

        let slot_count = tcp_listener_slot_count(backlog);
        let mut prepared_slots = Vec::with_capacity(slot_count.saturating_sub(1));
        for _ in 1..slot_count {
            prepared_slots.push(new_tcp_listener_socket(local_ip, port, tcp_nodelay)?);
        }

        self.sockets
            .get_mut::<tcp::Socket>(handle)
            .listen(listen_endpoint(local_ip, port))
            .map_err(|_| NetworkError::AddressInUse)?;

        let group_id = self.next_listener_group;
        self.next_listener_group = self.next_listener_group
                                       .wrapping_add(1)
                                       .max(1);
        let meta = self.socket_meta_mut(handle)?;
        meta.state = SocketState::Listening { port };
        meta.local_port = port;
        meta.is_listener = true;
        meta.listener_group = Some(group_id);

        let config = TcpListenerSlotConfig { group_id,
                                             local_ip,
                                             port,
                                             recv_timeout_ms,
                                             tcp_nodelay,
                                             snd_buf_size,
                                             rcv_buf_size };
        let mut handles = Vec::with_capacity(slot_count);
        handles.push(handle);
        for socket in prepared_slots {
            handles.push(self.register_tcp_listener_slot(socket, config));
        }
        self.tcp_listener_groups
            .insert(group_id, TcpListenerGroup { handles });
        Ok(())
    }

    fn accept(&mut self,
              handle : SocketHandle)
              -> Result<(SocketHandle, SocketHandle, [u8; 4], u16), NetworkError> {
        let config = {
            let meta = self.socket_meta(handle)?;
            if !meta.is_listener {
                return Err(NetworkError::NotListening);
            }
            let port = match meta.state {
                SocketState::Listening { port } => port,
                _ => return Err(NetworkError::NotListening),
            };
            TcpListenerSlotConfig { group_id : meta.listener_group
                                                   .ok_or(NetworkError::Internal)?,
                                    local_ip : meta.local_ip,
                                    port,
                                    recv_timeout_ms : meta.recv_timeout_ms,
                                    tcp_nodelay : meta.tcp_nodelay,
                                    snd_buf_size : meta.snd_buf_size,
                                    rcv_buf_size : meta.rcv_buf_size }
        };
        let listener_slots = self.tcp_listener_groups
                                 .get(&config.group_id)
                                 .ok_or(NetworkError::Internal)?
                                 .handles
                                 .clone();
        let established = listener_slots.into_iter()
                                        .find(|&slot| {
                                            tcp_is_accept_ready(self.sockets
                                                                    .get_mut::<tcp::Socket>(slot))
                                        })
                                        .ok_or(NetworkError::NoPendingConnection)?;
        let (peer_ip, peer_port) = {
            let socket = self.sockets
                             .get_mut::<tcp::Socket>(established);
            let remote = socket.remote_endpoint()
                               .ok_or(NetworkError::Internal)?;
            let peer_ip = match remote.addr {
                IpAddress::Ipv4(ip) => ip.octets(),
            };
            if peer_ip[0] == 127 {
                socket.set_nagle_enabled(false);
            }
            (peer_ip, remote.port)
        };

        let meta = self.socket_meta_mut(established)?;
        meta.state = SocketState::Connected;
        meta.connection_established = true;
        meta.connect_error = None;
        meta.connect_deadline_ms = None;
        meta.is_listener = false;
        meta.listener_group = None;
        meta.peer_ip = peer_ip;
        meta.peer_port = peer_port;
        meta.mcast_groups
            .clear();

        self.tcp_listener_groups
            .get_mut(&config.group_id)
            .ok_or(NetworkError::Internal)?
            .handles
            .retain(|&slot| slot != established);

        let new_listener = self.add_tcp_listener_slot(config)
                               .map_err(|_| NetworkError::Internal)?;
        self.tcp_listener_groups
            .get_mut(&config.group_id)
            .ok_or(NetworkError::Internal)?
            .handles
            .push(new_listener);

        let replacement = if established == handle {
            self.tcp_listener_groups
                .get(&config.group_id)
                .and_then(|group| {
                    group.handles
                         .first()
                })
                .copied()
                .ok_or(NetworkError::Internal)?
        } else {
            handle
        };
        Ok((established, replacement, peer_ip, peer_port))
    }
}

/// 创建 TCP socket，返回其 smoltcp 句柄。
pub fn create_tcp_socket() -> Result<SocketHandle, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   NetworkStack::create_tcp_socket)
}

/// TCP socket 开始监听（需先 bind）。
pub fn socket_listen(handle : SocketHandle, backlog : usize) -> Result<(), NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.listen(handle, backlog))
}

/// 从 listener 槽池取出一个已建立连接，并立即补充新的监听槽。
/// 返回 (已建立连接的 socket_handle, 新监听 socket_handle, 对端 IP, 对端端口)。
pub fn socket_accept(handle : SocketHandle)
                     -> Result<(SocketHandle, SocketHandle, [u8; 4], u16), NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.accept(handle))
}

#[cfg(test)]
mod tests {
    use super::{tcp_listener_slot_count, TCP_LISTEN_BACKLOG_MAX};

    #[test]
    fn listener_slot_count_honors_cagent_backlog() {
        assert_eq!(tcp_listener_slot_count(0), 2);
        assert_eq!(tcp_listener_slot_count(1), 2);
        assert_eq!(tcp_listener_slot_count(10), 11);
        assert_eq!(tcp_listener_slot_count(TCP_LISTEN_BACKLOG_MAX + 1),
                   TCP_LISTEN_BACKLOG_MAX);
    }
}
