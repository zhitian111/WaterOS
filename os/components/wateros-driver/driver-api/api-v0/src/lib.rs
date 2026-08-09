//! 设备驱动跨子系统共享的数据模型与错误类型。
//!
//! 用于 DTB 扫描阶段构造 [`DeviceInfo`]，并与各子系统 `supported_devices()` 声明做匹配；不包含具体 I/O trait。

#![no_std]
extern crate alloc;

pub mod dma;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

// Starts at one so a zero-initialized consumer cache is always stale.
static DEVICE_TOPOLOGY_GENERATION : AtomicU64 = AtomicU64::new(1);

/// Monotonic version of all driver registry membership.
///
/// Consumers use this as an invalidation hint and must still obtain a registry
/// snapshot for the actual devices. Counter wrap is practically unreachable;
/// equality remains the only supported comparison.
pub fn device_topology_generation() -> u64 {
    DEVICE_TOPOLOGY_GENERATION.load(Ordering::Acquire)
}

/// Notify registry consumers after a device becomes visible or invisible.
///
/// This is public because block/character/input/display API crates are separate
/// packages. Hardware drivers should call their subsystem registration API,
/// not this function directly.
#[doc(hidden)]
pub fn notify_device_topology_changed() -> u64 {
    DEVICE_TOPOLOGY_GENERATION.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
}

/// 由 MMIO 魔数与 VirtIO device id 等探测得到的设备大类，用于与子系统声明对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// VirtIO-MMIO 等设备探测为块设备（如 device id 2）。
    Block,
    /// 字符类设备（预留；当前 DTB 路径可能未填充）。
    Character,
    /// VirtIO 网络等（如 device id 1）。
    Network,
    /// 图形显示设备（当前为 VirtIO GPU，device id 16）。
    Display,
    /// 键盘、鼠标或平板输入设备（VirtIO device id 18）。
    Input,
    /// 非 virtio、魔数不匹配或未识别的节点。
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

/// 机器级驱动契约：引导接入、设备注册与可选平台能力。
///
/// 每个 `driver-impl` profile（QEMU RV/LA、dummy）实现本 trait，并通过
/// `machine()` 提供单例；上层只依赖该契约，不再直接引用具体 impl crate。
pub trait MachineDriver {
    /// 内核完成必要子系统初始化后调用：枚举并注册设备。
    fn init_after_boot(&self) -> DriverResult<()>;

    /// 平台可选能力（如实时钟）；不支持时默认返回 `Ok(None)`。
    fn realtime_ns(&self) -> DriverResult<Option<u64>> {
        Ok(None)
    }

    /// 驱动自检。
    fn test(&self);
}

/// 轻量自检：构造样例 [`DeviceInfo`] 并断言字段一致性；不访问 DTB 或硬件。
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
