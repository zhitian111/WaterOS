#![no_std]
//! 管道 v0 ring buffer 实现：固定容量 ring buffer + task waitqueue。
//!
//! `ARCH:` [`kernel_pipe::Pipe`] 保存共享字节流和两个等待队列；[`endpoint::PipeEndpoint`]
//! 提供 fd 可持有的方向与引用生命周期。实现 [`api_v0::KernelPipe`] 与
//! [`api_v0::PipeEndpointOps`]，不负责 fd 表或 syscall errno。

mod endpoint;
mod kernel_pipe;

pub use endpoint::PipeEndpoint;
pub(crate) use kernel_pipe::Pipe;

/// impl 层自检：创建最小 pipe 并验证非阻塞、端点 clone/close 与 EOF 语义。
pub fn test() {
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

    // 全关闭后空 pipe read 返回 EOF。
    let (read_eof, write_eof) = PipeEndpoint::pair(false);
    read_eof.close();
    write_eof.close();
    let mut eof_buf = [0u8; 4];
    assert_eq!(read_eof.read(&mut eof_buf), Ok(0));

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
}
