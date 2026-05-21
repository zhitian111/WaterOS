//! 网络协议栈烟测：TCP connect + HTTP 请求验证。
//!
//! [`spawn_all`] 在 poller 已启动后调用，任务跑完即退出。

use driver::network::stack;
use runtime::logging::*;

/// 尝试 TCP 连接到宿主机 10.0.2.2:80 并发 HTTP 请求。
extern "C" fn network_test_task(_arg: usize) -> ! {
    info!("[network-test] starting");

    // 等 poller 先跑至少一个周期
    task::sleep_for_ticks(1);

    info!("[network-test] TCP connecting to 10.0.2.2:80...");
    let _ = stack::tcp_connect([10, 0, 2, 2], 80);

    for round in 1..=8u32 {
        task::sleep_for_ticks(1);
        let active = stack::tcp_is_active();
        let may_send = stack::tcp_may_send();
        info!(
            "[network-test] round={} active={} may_send={}",
            round, active, may_send
        );
        if active && may_send {
            let _ = stack::tcp_send(b"GET / HTTP/1.0\r\nHost: 10.0.2.2\r\n\r\n");
            info!("[network-test] sent HTTP request");
            task::sleep_for_ticks(1);
            let mut buf = [0u8; 512];
            match stack::tcp_recv(&mut buf) {
                Ok(n) if n > 0 => {
                    info!("[network-test] recv {} bytes", n);
                    if let Ok(text) = core::str::from_utf8(&buf[..n.min(120)]) {
                        info!("[network-test] response: {}", text);
                    }
                }
                _ => info!("[network-test] no response (no HTTP server on host?)"),
            }
            break;
        }
    }
    info!("[network-test] done");
    task::exit_current(0);
}

/// 启动网络烟测任务。
pub fn spawn_all() {
    let id = task::spawn_kernel_task(network_test_task, 0);
    info!("[network-test] spawned test task={}", id);
}
