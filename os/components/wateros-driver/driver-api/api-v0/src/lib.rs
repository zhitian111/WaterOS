//! 设备驱动跨子系统共享的数据模型与错误类型。
//!
//! 用于 DTB 扫描阶段构造 [`DeviceInfo`]，并与各子系统 `supported_devices()` 声明做匹配；不包含具体 I/O trait。

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// 由 MMIO 魔数与 VirtIO device id 等探测得到的设备大类，用于与子系统声明对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Block,
    Character,
    Network,
    Unknown,
}

/// DTB `reg` 解析得到的一段 MMIO 物理区间（大小为 0 的条目应在调用方丢弃）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioRegion {
    /// 区域基址（物理）。
    pub base: usize,
    /// 区域长度（字节）。
    pub size: usize,
}

/// 设备节点上的中断描述（当前仅解析常见 `interrupts` / `interrupt-parent` 形态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqLine {
    /// 设备侧中断号（DTB 大端 u32 的首字）。
    pub irq: u32,
    /// 中断父节点 phandle，缺省为 `None`。
    pub parent: Option<u32>,
}

/// 子系统在 DTB 扫描阶段声明的「可绑定」设备描述（非排他；多个子系统可同时匹配同一 `compatible`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportedDeviceEntry {
    /// 子系统标识，例如 `"block"`。
    pub subsystem: &'static str,
    /// 人类可读名称，用于日志与诊断。
    pub name: &'static str,
    /// 与 DTB `compatible` 中字符串精确匹配的条目（单条，非列表）。
    pub compatible: &'static str,
}

/// 一次 DTB 节点扫描得到的摘要信息，供绑定决策与诊断使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// 节点名（含 `@` 单元地址等）。
    pub node_name: String,
    /// `compatible` 属性中的首条字符串（便于日志与兼容旧代码）。
    pub compatible: String,
    /// `compatible` 属性中的完整列表，用于与子系统 `supported_devices()` 做匹配。
    pub compatibles: Vec<String>,
    /// 探测到的设备类型；未识别 virtio 或非 virtio 节点可为 [`DeviceType::Unknown`]。
    pub device_type: DeviceType,
    /// 首个 `reg` MMIO 区域；无 `reg` 或非法时为 `None`。
    pub mmio: Option<MmioRegion>,
    /// 解析到的中断线；属性缺失或格式不支持时为 `None`。
    pub irq: Option<IrqLine>,
}

/// 驱动子系统与 DTB 解析共用的错误分类（不区分 errno 细节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// DTB 魔数或布局无效。
    InvalidDtb,
    /// 调用方参数或算术溢出等契约违反。
    InvalidParam,
    /// 所需资源不存在（例如尚未保存 DTB 基址）。
    NotFound,
    /// 硬件或传输层拒绝当前操作（含 feature 未启用路径）。
    Unsupported,
    /// 设备 I/O 失败（底层错误已折叠）。
    IoError,
}

/// [`DriverError`] 上的 [`Result`] 别名，便于块/字符等 API 统一签名。
pub type DriverResult<T> = core::result::Result<T, DriverError>;

/// 轻量自检：构造样例 [`DeviceInfo`] 并断言字段一致性。
pub fn test() {
    log::trace!("[driver-api] test begin");
    let info = DeviceInfo {
        node_name: String::from("virtio_blk@10001000"),
        compatible: String::from("virtio,mmio"),
        compatibles: Vec::from([String::from("virtio,mmio")]),
        device_type: DeviceType::Block,
        mmio: Some(MmioRegion {
            base: 0x1000_1000,
            size: 0x1000,
        }),
        irq: Some(IrqLine {
            irq: 1,
            parent: Some(0),
        }),
    };
    assert_eq!(info.device_type, DeviceType::Block);
    assert!(info.mmio.is_some());
    log::trace!("[driver-api] test end");
}
