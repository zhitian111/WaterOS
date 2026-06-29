//! 网络协议栈烟测：在调度器启动前同步验证核心 SocketManager API。
//!
//! [`run_sync_smoke`] 在 [`kernel_main`] 网络栈初始化后立即调用。

use driver::network::stack;
use runtime::logging::*;

#[inline]
fn drive_stack(now : &mut i64, rounds : i64) {
    for _ in 0..rounds {
        *now += 1;
        stack::poll_at_millis(*now);
        stack::poll_socket_events();
    }
}

fn wait_accept(now : &mut i64,
               listener : stack::StackSocketHandle,
               client : stack::StackSocketHandle)
               -> Option<(stack::StackSocketHandle, stack::StackSocketHandle)> {
    for _ in 0..128 {
        drive_stack(now, 1);
        let client_ready = stack::socket_is_connected(client).unwrap_or(false);
        let server_ready = stack::socket_has_pending_accept(listener).unwrap_or(false);
        if client_ready && server_ready {
            match stack::socket_accept(listener) {
                Ok((accepted, replacement, _port)) => return Some((accepted, replacement)),
                Err(e) => {
                    warn!("[socket-smoke] accept failed after ready: {}",
                          e);
                    return None;
                }
            }
        }
    }
    None
}

#[inline]
fn wait_may_send(now : &mut i64, socket : stack::StackSocketHandle) -> bool {
    for _ in 0..64 {
        drive_stack(now, 1);
        if stack::socket_may_send(socket).unwrap_or(false) {
            return true;
        }
    }
    false
}

/// 同步烟测（调度器启动前调用，不依赖任务系统）。
/// 验证核心 SocketManager API（创建/绑定/监听/连接）是否正常。
pub fn run_sync_smoke() {
    info!("[socket-smoke] synchronous smoke test begin");
    let mut now = 0;

    // 1. TCP socket 创建 + bind + listen
    let server = match stack::create_tcp_socket() {
        Ok(h) => {
            info!("[socket-smoke] TCP server created, handle={:?}",
                  h);
            h
        }
        Err(e) => {
            warn!("[socket-smoke] TCP server create failed: {}",
                  e);
            return;
        }
    };

    match stack::socket_bind(server, None, 12345) {
        Ok(()) => info!("[socket-smoke] TCP server bind 0.0.0.0:12345 ok"),
        Err(e) => warn!("[socket-smoke] TCP server bind failed: {}",
                        e),
    }

    match stack::socket_listen(server) {
        Ok(()) => info!("[socket-smoke] TCP server listen ok"),
        Err(e) => warn!("[socket-smoke] TCP server listen failed: {}",
                        e),
    }

    match stack::socket_state(server) {
        Ok(s) => info!("[socket-smoke] server state={:?}", s),
        Err(e) => warn!("[socket-smoke] server state err: {}", e),
    }

    // 2. 客户端 connect — 127.0.0.1 loopback
    let client = match stack::create_tcp_socket() {
        Ok(h) => {
            info!("[socket-smoke] TCP client created, handle={:?}",
                  h);
            h
        }
        Err(e) => {
            warn!("[socket-smoke] TCP client create failed: {}",
                  e);
            let _ = stack::socket_close(server);
            return;
        }
    };

    match stack::socket_connect(client, [127, 0, 0, 1], 12345) {
        Ok(()) => info!("[socket-smoke] TCP connect to 127.0.0.1:12345 ok"),
        Err(e) => warn!("[socket-smoke] TCP connect to 127.0.0.1:12345 failed: {}",
                        e),
    }

    let (accepted, replacement_listener) = match wait_accept(&mut now, server, client) {
        Some(v) => {
            info!("[socket-smoke] TCP loopback accept ok");
            v
        }
        None => {
            warn!("[socket-smoke] TCP loopback accept timeout");
            let _ = stack::socket_close(client);
            let _ = stack::socket_close(server);
            return;
        }
    };

    if !wait_may_send(&mut now, client) {
        warn!("[socket-smoke] TCP client may_send timeout");
    }
    match stack::socket_send(client, b"ping") {
        Ok(n) => info!("[socket-smoke] TCP client send {} bytes",
                       n),
        Err(e) => warn!("[socket-smoke] TCP client send failed: {}",
                        e),
    }
    drive_stack(&mut now, 16);
    let mut buf = [0u8; 16];
    match stack::socket_recv(accepted, &mut buf) {
        Ok(4) if &buf[..4] == b"ping" => info!("[socket-smoke] TCP server recv ping ok"),
        Ok(n) => warn!("[socket-smoke] TCP server recv unexpected {} bytes",
                       n),
        Err(e) => warn!("[socket-smoke] TCP server recv failed: {}",
                        e),
    }

    match stack::socket_send(accepted, b"pong") {
        Ok(n) => info!("[socket-smoke] TCP server send {} bytes",
                       n),
        Err(e) => warn!("[socket-smoke] TCP server send failed: {}",
                        e),
    }
    drive_stack(&mut now, 16);
    match stack::socket_recv(client, &mut buf) {
        Ok(4) if &buf[..4] == b"pong" => info!("[socket-smoke] TCP client recv pong ok"),
        Ok(n) => warn!("[socket-smoke] TCP client recv unexpected {} bytes",
                       n),
        Err(e) => warn!("[socket-smoke] TCP client recv failed: {}",
                        e),
    }

    // 3. UDP socket 创建
    match stack::create_udp_socket() {
        Ok(h) => {
            info!("[socket-smoke] UDP socket created, handle={:?}",
                  h);
            let _ = stack::socket_close(h);
        }
        Err(e) => warn!("[socket-smoke] UDP socket create failed: {}",
                        e),
    }

    // 清理
    let _ = stack::socket_close(replacement_listener);
    let _ = stack::socket_close(accepted);
    let _ = stack::socket_close(client);

    // 手动 poll 一轮
    drive_stack(&mut now, 1);
    stack::poll_socket_events();

    info!("[socket-smoke] synchronous smoke test end");
}

/// 网络烟测入口（当前为空；核心验证已通过 [`run_sync_smoke`] 完成）。
pub fn spawn_all() {}
