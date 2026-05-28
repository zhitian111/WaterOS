//! 网络协议栈烟测：在调度器启动前同步验证核心 SocketManager API。
//!
//! [`run_sync_smoke`] 在 [`kernel_main`] 网络栈初始化后立即调用。

use driver::network::stack;
use runtime::logging::*;

/// 同步烟测（调度器启动前调用，不依赖任务系统）。
/// 验证核心 SocketManager API（创建/绑定/监听/连接）是否正常。
pub fn run_sync_smoke() {
    info!("[socket-smoke] synchronous smoke test begin");

    // 1. TCP socket 创建 + bind + listen
    let server = match stack::create_tcp_socket() {
        Ok(h) => {
            info!("[socket-smoke] TCP server created, handle={:?}", h);
            h
        }
        Err(e) => {
            warn!("[socket-smoke] TCP server create failed: {}", e);
            return;
        }
    };

    match stack::socket_bind(server, 12345) {
        Ok(()) => info!("[socket-smoke] TCP server bind port=12345 ok"),
        Err(e) => warn!("[socket-smoke] TCP server bind failed: {}", e),
    }

    match stack::socket_listen(server) {
        Ok(()) => info!("[socket-smoke] TCP server listen ok"),
        Err(e) => warn!("[socket-smoke] TCP server listen failed: {}", e),
    }

    match stack::socket_state(server) {
        Ok(s) => info!("[socket-smoke] server state={:?}", s),
        Err(e) => warn!("[socket-smoke] server state err: {}", e),
    }

    // 2. 客户端 connect — 127.0.0.1 loopback
    let client = match stack::create_tcp_socket() {
        Ok(h) => {
            info!("[socket-smoke] TCP client created, handle={:?}", h);
            h
        }
        Err(e) => {
            warn!("[socket-smoke] TCP client create failed: {}", e);
            let _ = stack::socket_close(server);
            return;
        }
    };

    match stack::socket_connect(client, [127, 0, 0, 1], 12345) {
        Ok(()) => info!("[socket-smoke] TCP connect to 127.0.0.1:12345 ok"),
        Err(e) => warn!("[socket-smoke] TCP connect to 127.0.0.1:12345 failed: {}", e),
    }

    // 3. 客户端 connect — 10.0.2.15（接口自身 IP）
    let client2 = match stack::create_tcp_socket() {
        Ok(h) => {
            info!("[socket-smoke] TCP client2 created, handle={:?}", h);
            h
        }
        Err(e) => {
            warn!("[socket-smoke] TCP client2 create failed: {}", e);
            let _ = stack::socket_close(client);
            let _ = stack::socket_close(server);
            return;
        }
    };

    match stack::socket_connect(client2, [10, 0, 2, 15], 12345) {
        Ok(()) => info!("[socket-smoke] TCP connect to 10.0.2.15:12345 ok"),
        Err(e) => warn!("[socket-smoke] TCP connect to 10.0.2.15:12345 failed: {}", e),
    }

    // 4. UDP socket 创建
    match stack::create_udp_socket() {
        Ok(h) => {
            info!("[socket-smoke] UDP socket created, handle={:?}", h);
            let _ = stack::socket_close(h);
        }
        Err(e) => warn!("[socket-smoke] UDP socket create failed: {}", e),
    }

    // 清理
    let _ = stack::socket_close(client2);
    let _ = stack::socket_close(client);
    let _ = stack::socket_close(server);

    // 手动 poll 一轮
    stack::poll();
    stack::poll_socket_events();

    info!("[socket-smoke] synchronous smoke test end");
}

/// 网络烟测入口（当前为空；核心验证已通过 [`run_sync_smoke`] 完成）。
pub fn spawn_all() {}
