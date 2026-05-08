//! 字符设备 API（v0）：串口等 **阻塞 / 非阻塞** 字节 I/O trait，与具体 MMIO 实现解耦。

#![no_std]

/// 串行端口一次字节写入失败原因（忙等路径上多为发送保持寄存器未空）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialError {
    /// 发送侧在合理自旋后仍不可写（硬件异常或误用寄存器布局）。
    TransmitterStuck,
}

/// 串行端口上的 `Result` 别名。
pub type SerialResult<T> = core::result::Result<T, SerialError>;

/// 最小串行 I/O 契约：伪 shell 与 bring-up 调试使用；**不**规定线程安全，由实现与调用方保证。
pub trait SerialPort {
    /// 写单字节；发送前可自旋直至 THRE。
    fn write_byte(&mut self, byte : u8) -> SerialResult<()>;

    /// 顺序写入缓冲区。
    fn write_all(&mut self, bytes : &[u8]) -> SerialResult<()> {
        for &b in bytes {
            self.write_byte(b)?;
        }
        Ok(())
    }

    /// 阻塞直到收到一字节（自旋轮询 DR）。
    fn read_byte_blocking(&mut self) -> u8;

    /// 无数据时返回 `None`，不阻塞。
    fn try_read_byte(&mut self) -> Option<u8>;
}
