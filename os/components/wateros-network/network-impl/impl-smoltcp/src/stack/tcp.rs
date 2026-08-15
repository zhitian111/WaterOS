//! TCP socket 创建、监听与 accept 槽池管理。

use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp;
use smoltcp::time::Duration;

use super::global::with_stack_mut;
#[cfg(feature = "ipv6")]
use super::socket::loopback_address;
use super::socket::{listen_endpoint, network_address};
use super::state::{
    NetworkStack, SocketMeta, TcpListenerGroup, TCP_LISTEN_BACKLOG_MAX, TCP_RX_BUFFER_SIZE,
    TCP_TX_BUFFER_SIZE,
};
use super::types::{
    NetworkAddress, NetworkEndpoint, NetworkError, SocketDomain, SocketKind, SocketState,
};

#[derive(Clone, Copy)]
struct TcpListenerSlotConfig {
    group_id : u64,
    domain : SocketDomain,
    local_ip : Option<NetworkAddress>,
    listen_ip : Option<NetworkAddress>,
    port : u16,
    recv_timeout_ms : Option<u64>,
    tcp_nodelay : bool,
    snd_buf_size : i32,
    rcv_buf_size : i32,
}

pub(super) fn tcp_listener_slot_count(backlog : usize) -> usize {
    // Linux defines backlog as the queue of fully established connections
    // still waiting for accept(). The connection currently being accepted
    // is not part of that queue. Keep one transition slot in addition to
    // the requested queue depth so a replacement listener is available
    // while a userspace server handles the accepted connection.
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

fn new_tcp_listener_socket(local_ip : Option<NetworkAddress>,
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
    fn create_tcp_socket(&mut self, domain : SocketDomain) -> Result<SocketHandle, NetworkError> {
        #[cfg(not(feature = "ipv6"))]
        if domain == SocketDomain::Ipv6 {
            return Err(NetworkError::Unsupported);
        }
        let handle = self.sockets
                         .add(new_tcp_socket());
        self.metas
            .insert(handle,
                    SocketMeta::new(domain, SocketKind::Tcp));
        Ok(handle)
    }

    fn register_tcp_listener_slot(&mut self,
                                  socket : tcp::Socket<'static>,
                                  config : TcpListenerSlotConfig)
                                  -> SocketHandle {
        let handle = self.sockets
                         .add(socket);
        let mut meta = SocketMeta::new(config.domain, SocketKind::Tcp);
        meta.state = SocketState::Listening { port : config.port };
        meta.local_ip = config.local_ip;
        meta.listen_ip = config.listen_ip;
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
        let socket = new_tcp_listener_socket(config.listen_ip,
                                             config.port,
                                             config.tcp_nodelay)?;
        Ok(self.register_tcp_listener_slot(socket, config))
    }

    fn listen(&mut self, handle : SocketHandle, backlog : usize) -> Result<(), NetworkError> {
        let (domain, mut port, local_ip, recv_timeout_ms, tcp_nodelay, snd_buf_size, rcv_buf_size) = {
            let meta = self.socket_meta(handle)?;
            if meta.kind != SocketKind::Tcp {
                return Err(NetworkError::WrongSocketType);
            }
            let port = match meta.state {
                SocketState::Bound { port } => port,
                _ => return Err(NetworkError::NotBound),
            };
            (meta.domain,
             port,
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

        let mut listen_ips = Vec::with_capacity(2);
        #[cfg(feature = "ipv6")]
        {
            if let Some(local_ip) = local_ip {
                listen_ips.push(Some(local_ip));
            } else {
                if let Some(configured) = self.configured_address(domain) {
                    listen_ips.push(Some(configured));
                }
                let loopback = loopback_address(domain);
                if !listen_ips.contains(&Some(loopback)) {
                    listen_ips.push(Some(loopback));
                }
            }
        }
        #[cfg(not(feature = "ipv6"))]
        {
            // IPv4-only 时 None 对应原来的 0.0.0.0 wildcard，所有本机
            // IPv4 地址动态共享完整 backlog。
            listen_ips.push(local_ip);
        }

        let group_id = self.next_listener_group;
        self.next_listener_group = self.next_listener_group
                                       .wrapping_add(1)
                                       .max(1);
        let slots_per_ip = tcp_listener_slot_count(backlog);
        let total_slots = slots_per_ip.saturating_mul(listen_ips.len());
        let mut prepared_slots = Vec::with_capacity(total_slots.saturating_sub(1));
        for (ip_index, listen_ip) in listen_ips.iter()
                                               .copied()
                                               .enumerate()
        {
            for slot_index in 0..slots_per_ip {
                // 原 socket 作为第一个地址的第一个监听槽继续使用。
                if ip_index == 0 && slot_index == 0 {
                    continue;
                }
                let config = TcpListenerSlotConfig { group_id,
                                                     domain,
                                                     local_ip,
                                                     listen_ip,
                                                     port,
                                                     recv_timeout_ms,
                                                     tcp_nodelay,
                                                     snd_buf_size,
                                                     rcv_buf_size };
                prepared_slots.push((new_tcp_listener_socket(config.listen_ip,
                                                             port,
                                                             tcp_nodelay)?,
                                     config));
            }
        }

        let first_listen_ip = listen_ips[0];
        self.sockets
            .get_mut::<tcp::Socket>(handle)
            .listen(listen_endpoint(first_listen_ip, port))
            .map_err(|_| NetworkError::AddressInUse)?;

        let meta = self.socket_meta_mut(handle)?;
        meta.state = SocketState::Listening { port };
        meta.local_port = port;
        meta.listen_ip = first_listen_ip;
        meta.is_listener = true;
        meta.listener_group = Some(group_id);

        let mut handles = Vec::with_capacity(total_slots);
        handles.push(handle);
        for (socket, config) in prepared_slots {
            handles.push(self.register_tcp_listener_slot(socket, config));
        }
        self.tcp_listener_groups
            .insert(group_id, TcpListenerGroup { handles });
        Ok(())
    }

    fn accept(&mut self,
              handle : SocketHandle)
              -> Result<(SocketHandle, SocketHandle, NetworkEndpoint), NetworkError> {
        let group_id = {
            let meta = self.socket_meta(handle)?;
            if !meta.is_listener {
                return Err(NetworkError::NotListening);
            }
            match meta.state {
                SocketState::Listening { .. } => {}
                _ => return Err(NetworkError::NotListening),
            }
            meta.listener_group
                .ok_or(NetworkError::Internal)?
        };
        let listener_slots = self.tcp_listener_groups
                                 .get(&group_id)
                                 .ok_or(NetworkError::Internal)?
                                 .handles
                                 .clone();
        let established = listener_slots.into_iter()
                                        .find(|&slot| {
                                            tcp_is_accept_ready(self.sockets
                                                                    .get_mut::<tcp::Socket>(slot))
                                        })
                                        .ok_or(NetworkError::NoPendingConnection)?;
        let config = {
            let meta = self.socket_meta(established)?;
            let port = match meta.state {
                SocketState::Listening { port } => port,
                _ => return Err(NetworkError::Internal),
            };
            TcpListenerSlotConfig { group_id,
                                    domain : meta.domain,
                                    local_ip : meta.local_ip,
                                    listen_ip : meta.listen_ip,
                                    port,
                                    recv_timeout_ms : meta.recv_timeout_ms,
                                    tcp_nodelay : meta.tcp_nodelay,
                                    snd_buf_size : meta.snd_buf_size,
                                    rcv_buf_size : meta.rcv_buf_size }
        };
        let (peer_ip, peer_port) = {
            let socket = self.sockets
                             .get_mut::<tcp::Socket>(established);
            let remote = socket.remote_endpoint()
                               .ok_or(NetworkError::Internal)?;
            let peer_ip = network_address(remote.addr);
            if peer_ip.is_loopback() {
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
        meta.listen_ip = None;
        meta.peer_ip = peer_ip;
        meta.peer_port = peer_port;
        meta.mcast_groups
            .clear();

        self.tcp_listener_groups
            .get_mut(&group_id)
            .ok_or(NetworkError::Internal)?
            .handles
            .retain(|&slot| slot != established);

        let new_listener = self.add_tcp_listener_slot(config)
                               .map_err(|_| NetworkError::Internal)?;
        self.tcp_listener_groups
            .get_mut(&group_id)
            .ok_or(NetworkError::Internal)?
            .handles
            .push(new_listener);

        let replacement = if established == handle {
            self.tcp_listener_groups
                .get(&group_id)
                .and_then(|group| {
                    group.handles
                         .first()
                })
                .copied()
                .ok_or(NetworkError::Internal)?
        } else {
            handle
        };
        Ok((established,
            replacement,
            NetworkEndpoint { address : peer_ip,
                              port : peer_port,
                              scope_id : 0 }))
    }
}

/// 创建 TCP socket，返回其 smoltcp 句柄。
pub fn create_tcp_socket(domain : SocketDomain) -> Result<SocketHandle, NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.create_tcp_socket(domain))
}

/// TCP socket 开始监听（需先 bind）。
pub fn socket_listen(handle : SocketHandle, backlog : usize) -> Result<(), NetworkError> {
    with_stack_mut(NetworkError::StackUnavailable,
                   |stack| stack.listen(handle, backlog))
}

/// 从 listener 槽池取出一个已建立连接，并立即补充新的监听槽。
/// 返回 (已建立连接的 socket_handle, 新监听 socket_handle, 对端端点)。
pub fn socket_accept(handle : SocketHandle)
                     -> Result<(SocketHandle, SocketHandle, NetworkEndpoint), NetworkError> {
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
