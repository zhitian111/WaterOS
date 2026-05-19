#![no_std]
//! 管道 v0 ring buffer 实现：固定容量 ring buffer + task waitqueue。
//!
//! 行为为可用的内核内部 pipe，实现 [`api_v0::KernelPipe`] 与 [`api_v0::PipeEndpointOps`]。

mod endpoint;
mod kernel_pipe;

pub use endpoint::PipeEndpoint;
pub use kernel_pipe::Pipe;

/// impl 层自检：创建最小 pipe 并验证阻塞/非阻塞契约。
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
}
