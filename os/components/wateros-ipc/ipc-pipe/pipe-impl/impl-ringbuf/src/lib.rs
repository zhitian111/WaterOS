#![no_std]
//! 管道 v0 ring buffer 实现：固定容量 ring buffer + task waitqueue。
//!
//! `ARCH:` [`kernel_pipe::Pipe`] 保存共享字节流和两个等待队列；[`endpoint::PipeEndpoint`]
//! 提供 fd 可持有的方向与引用生命周期。实现 [`api_v0::KernelPipe`] 与
//! [`api_v0::PipeEndpointOps`]，不负责 fd 表或 syscall errno。

extern crate alloc;

mod endpoint;
mod kernel_pipe;

pub use endpoint::{NamedPipe, PipeEndpoint};
pub(crate) use kernel_pipe::Pipe;

/// impl 层自检：创建最小 pipe 并验证非阻塞、端点 clone/close 与 EOF 语义。
pub fn test() {
    use alloc::sync::Arc;

    let pipe = Pipe::with_capacity(8).expect("pipe capacity should be valid");
    assert_eq!(pipe.capacity(), 8);
    assert_eq!(pipe.len(), 0);

    let mut buf = [0u8; 2];
    assert_eq!(
        pipe.try_read(&mut buf),
        Err(api_v0::PipeError::WouldBlock)
    );
    assert_eq!(pipe.try_write(&[1, 2]), Ok(2));
    assert_eq!(pipe.len(), 2);
    assert_eq!(pipe.read(&mut buf), Ok(2));
    assert_eq!(&buf, &[1, 2]);

    let (read_end, write_end) = PipeEndpoint::pair(false);
    assert_eq!(read_end.kind(), api_v0::PipeEndpointKind::Read);
    assert_eq!(write_end.kind(), api_v0::PipeEndpointKind::Write);

    // dup/fork clone 共享同一个端点的 OFD status；另一方向是独立 OFD。
    let read_dup = read_end.clone();
    read_dup.set_nonblocking(true);
    read_dup.set_direct(true);
    assert!(read_end.nonblocking());
    assert!(read_end.direct());
    assert!(!write_end.nonblocking());
    assert!(!write_end.direct());

    // 部分 close 仍可读：clone 读端 → close 原读端 → 写端 write → clone 读端 read。
    let (read_a, write_a) = PipeEndpoint::pair(false);
    let read_b = read_a.clone();
    read_a.close();
    assert_eq!(write_a.write(b"ab"), Ok(2));
    let mut buf = [0u8; 2];
    assert_eq!(read_b.read(&mut buf), Ok(2));
    assert_eq!(&buf, b"ab");
    read_b.close();
    write_a.close();

    // 部分 close 仍可写：clone 写端 → close 原写端 → 保留写端 write → 读端 read。
    let (read_b2, write_b2) = PipeEndpoint::pair(false);
    let write_c = write_b2.clone();
    write_b2.close();
    assert_eq!(write_c.write(b"xy"), Ok(2));
    let mut buf2 = [0u8; 2];
    assert_eq!(read_b2.read(&mut buf2), Ok(2));
    assert_eq!(&buf2, b"xy");
    read_b2.close();
    write_c.close();

    // Shell 管道模拟：子 close 读 fd、父 close 写 fd 后，子 stdout 写 / 父 stdin 读仍通。
    let (r_parent, w_child) = PipeEndpoint::pair(false);
    let w_stdout = w_child.clone();
    let r_stdin = r_parent.clone();
    r_parent.close();
    w_child.close();
    assert_eq!(w_stdout.write(b"line1\n"), Ok(6));
    let mut line = [0u8; 6];
    assert_eq!(r_stdin.read(&mut line), Ok(6));
    assert_eq!(&line, b"line1\n");
    r_stdin.close();
    w_stdout.close();

    // 最后一个写端关闭后，仍打开的空读端返回 EOF。
    let (read_eof, write_eof) = PipeEndpoint::pair(false);
    write_eof.close();
    let mut eof_buf = [0u8; 4];
    assert_eq!(read_eof.read(&mut eof_buf), Ok(0));
    read_eof.close();

    // 未显式 close 的端点也必须在析构时释放引用并唤醒/通知另一端。
    let (read_drop, write_after_drop) = PipeEndpoint::pair(false);
    drop(read_drop);
    assert_eq!(write_after_drop.write(b"x"),
               Err(api_v0::PipeError::BrokenPipe));

    let (read_after_drop, write_drop) = PipeEndpoint::pair(false);
    drop(write_drop);
    let mut dropped_eof = [0u8; 1];
    assert_eq!(read_after_drop.read(&mut dropped_eof), Ok(0));

    // close 后即使端点对象仍被内核暂存，也不能再代表一个可操作 fd。
    let (closed_read, closed_write) = PipeEndpoint::pair(false);
    closed_read.close();
    let mut closed_buf = [0u8; 1];
    assert_eq!(closed_read.read(&mut closed_buf),
               Err(api_v0::PipeError::Closed));
    closed_write.close();
    assert_eq!(closed_write.write(b"x"),
               Err(api_v0::PipeError::Closed));

    // A filesystem FIFO has no hidden sentinel endpoints: nonblocking writer
    // open requires a reader, and closing the final writer exposes EOF.
    let fifo = NamedPipe::new();
    assert_eq!(fifo.open_write(true).err(),
               Some(api_v0::PipeError::BrokenPipe));
    let fifo_read = fifo.open_read(true).expect("open named pipe reader");
    let mut fifo_buf = [0u8; 1];
    assert_eq!(fifo_read.read(&mut fifo_buf), Ok(0));
    let fifo_write = fifo.open_write(true).expect("open named pipe writer");
    assert_eq!(fifo_read.read(&mut fifo_buf),
               Err(api_v0::PipeError::WouldBlock));
    assert_eq!(fifo_write.write(b"n"), Ok(1));
    assert_eq!(fifo_read.read(&mut fifo_buf), Ok(1));
    assert_eq!(&fifo_buf, b"n");
    fifo_write.close();
    assert_eq!(fifo_read.read(&mut fifo_buf), Ok(0));
    fifo_read.close();
    assert_eq!(fifo.open_write(true).err(),
               Some(api_v0::PipeError::BrokenPipe));

    // stream reservation 只提交到达用户空间的前缀。
    let stream = Arc::new(Pipe::with_capacity(6).expect("reservation pipe"));
    assert_eq!(stream.try_write(b"abcdef"), Ok(6));
    let lease = stream
        .acquire_read_lease(6, false)
        .expect("stream lease");
    assert_eq!(lease.bytes(), b"abcdef");
    assert_eq!(
        lease.finish(3, false),
        Ok(api_v0::PipeReadFinish::Bytes(3))
    );
    let lease = stream
        .acquire_read_lease(6, false)
        .expect("stream remainder");
    assert_eq!(lease.bytes(), b"def");
    assert_eq!(
        lease.finish(0, false),
        Ok(api_v0::PipeReadFinish::Fault)
    );
    let lease = stream
        .acquire_read_lease(6, false)
        .expect("stream zero-fault rollback");
    assert_eq!(lease.bytes(), b"def");
    drop(lease);
    let lease = stream
        .acquire_read_lease(6, false)
        .expect("stream drop rollback");
    assert_eq!(lease.bytes(), b"def");
    assert_eq!(
        stream.acquire_read_lease(1, true).err(),
        Some(api_v0::PipeError::WouldBlock)
    );
    assert_eq!(
        lease.finish(3, true),
        Ok(api_v0::PipeReadFinish::Bytes(3))
    );

    let capacity = Arc::new(Pipe::with_capacity(3).expect("capacity pipe"));
    assert_eq!(capacity.try_write(b"abc"), Ok(3));
    let lease = capacity
        .acquire_read_lease(3, false)
        .expect("capacity lease");
    assert_eq!(
        capacity.try_write(b"x"),
        Err(api_v0::PipeError::WouldBlock)
    );
    drop(lease);

    // packet mode 截断一次读取，但不会与后一 packet 合并。
    let packet = Arc::new(Pipe::with_capacity(8).expect("packet pipe"));
    assert_eq!(packet.try_write_mode(b"abcdef", true), Ok(6));
    assert_eq!(packet.try_write_mode(b"xy", true), Ok(2));
    let lease = packet
        .acquire_read_lease(3, false)
        .expect("packet lease");
    assert_eq!(lease.bytes(), b"abc");
    assert_eq!(
        lease.finish(3, true),
        Ok(api_v0::PipeReadFinish::Bytes(3))
    );
    let lease = packet
        .acquire_read_lease(8, false)
        .expect("next packet");
    assert_eq!(lease.bytes(), b"xy");
    assert_eq!(
        lease.finish(2, true),
        Ok(api_v0::PipeReadFinish::Bytes(2))
    );
    assert_eq!(packet.try_write_mode(b"fault", true), Ok(5));
    assert_eq!(packet.try_write_mode(b"ok", true), Ok(2));
    let lease = packet
        .acquire_read_lease(5, false)
        .expect("packet partial-fault lease");
    assert_eq!(
        lease.finish(2, false),
        Ok(api_v0::PipeReadFinish::Fault)
    );
    let lease = packet
        .acquire_read_lease(5, false)
        .expect("packet after partial fault");
    assert_eq!(lease.bytes(), b"ok");
    assert_eq!(
        lease.finish(2, true),
        Ok(api_v0::PipeReadFinish::Bytes(2))
    );

    // writer close 后，已保留数据先提交，随后才观察到 EOF。
    let (read_close, write_close) = PipeEndpoint::pair(false);
    assert_eq!(write_close.write(b"z"), Ok(1));
    let lease = read_close
        .acquire_read_lease(1)
        .expect("close lease");
    write_close.close();
    assert_eq!(
        lease.finish(1, true),
        Ok(api_v0::PipeReadFinish::Bytes(1))
    );
    let eof = read_close
        .acquire_read_lease(1)
        .expect("close eof");
    assert!(eof.bytes().is_empty());
    assert_eq!(
        eof.finish(0, true),
        Ok(api_v0::PipeReadFinish::Bytes(0))
    );

    // 最后一个读端关闭会撤销 active reservation，旧 lease 不得在关闭后提交。
    let (read_cancel, write_cancel) = PipeEndpoint::pair(false);
    assert_eq!(write_cancel.write(b"q"), Ok(1));
    let lease = read_cancel
        .acquire_read_lease(1)
        .expect("close-cancel lease");
    read_cancel.close();
    assert_eq!(
        lease.finish(1, true),
        Err(api_v0::PipeError::Closed)
    );
    assert_eq!(
        write_cancel.write(b"r"),
        Err(api_v0::PipeError::BrokenPipe)
    );
}
