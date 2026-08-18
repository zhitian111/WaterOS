#![no_std]
//! klog 的内核实现：记录上下文、格式化和 `sys_syslog` 缓冲语义。
//!
//! `ARCH:` 本 crate 连接 API 契约、ringbuf 存储、platform 时间与 task 上下文。
//! 聚合 crate 仅重导出这里的稳定入口；用户地址验证和 `copy_{to,from}_user` 仍属于
//! `wateros-syscall`。

mod format;
mod global;
mod state;
mod syslog;

/// 将格式化参数写入内核日志环的无分配入口。
pub use format::record_fmt;
/// 初始化、记录、统计和内核缓冲 syslog 分发入口。
pub use global::{dispatch_kernel, init, record, stats};

#[cfg(feature = "self_test")]
/// 写入并统计一条记录，验证最小全局服务链路；会清空已有日志，只能用于显式自测场景。
pub fn self_test() {
    use api_v0::{LOG_INFO, LOG_KERN};
    init();
    let _result = record(LOG_INFO, LOG_KERN, b"klog-self-test");
    assert!(stats().records_committed >= 1);
}

#[cfg(test)]
mod tests {
    extern crate std;

    use crate::global::KlogRingbuf;
    use api_v0::{KlogFlags, KlogRecordMeta, KlogStore, LOG_INFO, LOG_KERN};

    /// 基本环语义：追加后可读，推进 cursor 后不再计入 unread 字节。
    #[test]
    fn append_and_advance_read_cursor() {
        KlogRingbuf::init();
        KlogRingbuf::with(|ring| {
            let mut meta = KlogRecordMeta::new(0, 0, LOG_KERN, LOG_INFO, KlogFlags::empty(), 0);
            ring.append(&mut meta, b"hello");
            assert_eq!(ring.unread_bytes(), 5);
            let view = ring.peek_next_unread().unwrap();
            assert_eq!(view.text, b"hello");
            ring.advance_read_cursor(view.meta.seq);
            assert_eq!(ring.unread_bytes(), 0);
        });
    }
}
