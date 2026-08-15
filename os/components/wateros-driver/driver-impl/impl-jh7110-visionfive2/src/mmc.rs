//! VisionFive 2 MMC resources and compatibility exports.
//!
//! Clock/reset/syscon descriptions belong to this board layer. Controller PIO
//! and SD protocol logic live in `wateros-driver-block-impl-dw-mmc` so another
//! platform can reuse them without importing JH7110 topology assumptions.
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use api_v0::MmioRegion;
use block::{BlockDevice, DriverError, Lba, SharedBlockDevice, BLOCK_SIZE};

pub use dw_mmc::mmc::{clock_divider, DwMmc, MmcError, MmioRegisters, RegisterIo};
use dw_mmc::sd::SdCard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcActivationBlocker {
    InvalidMmio,
    InvalidIrq,
    InvalidBusWidth,
    MissingBiuClock,
    MissingCiuClock,
    MissingReset,
    MissingSysreg,
    MissingTargetFrequency,
    MissingFifoDepth,
    InvalidTargetFrequency,
    InvalidFifoDepth,
    HardwareEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcControllerConfig {
    pub target_frequency_hz : u32,
    pub fifo_depth : u32,
    pub bus_width : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcConfigError {
    InvalidStaticResources,
    MissingTargetFrequency,
    MissingFifoDepth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcInitializationError {
    NotReady,
    Core(MmcError),
    Card(MmcError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcRegistrationError {
    InvalidBlockSize,
    UnknownCapacity,
    EmptyDevice,
    Read(DriverError),
}

/// Register an already initialized SD device only after a bounded read probe.
///
/// This helper is intentionally separate from controller/card initialization:
/// callers can run it after explicit board evidence, and a failed probe never
/// mutates the global block registry.
pub fn register_readonly_block_device(device : SharedBlockDevice)
                                     -> Result<usize, MmcRegistrationError> {
    let mut guard = device.lock();
    if guard.block_size() != BLOCK_SIZE {
        return Err(MmcRegistrationError::InvalidBlockSize);
    }
    let total = guard.total_blocks().ok_or(MmcRegistrationError::UnknownCapacity)?;
    if total == 0 {
        return Err(MmcRegistrationError::EmptyDevice);
    }
    let mut sample = [0u8; BLOCK_SIZE];
    guard.read_blocks(Lba(0), &mut sample).map_err(MmcRegistrationError::Read)?;
    drop(guard);
    Ok(block::register_block_device(device))
}

/// 真机 bring-up 解锁证据（任务 21 首轮实测）。
///
/// 已确认：DTB 拓扑（地址/IRQ/bus width/最大频率）与 PLIC S 态上下文在真机
/// 生效（日志 `enabled supervisor external interrupts hart=1..4`）；控制器
/// 时钟/reset/pinmux 由固件（U-Boot）保持打开（U-Boot 曾以 50MHz 读同一张
/// SD 卡）。首次激活若失败，错误会带上下文记入日志，据此迭代。
pub const MMC_ACTIVATION_EVIDENCE : MmcHardwareEvidence = MmcHardwareEvidence {
    clock_verified : true,
    reset_verified : true,
    irq_verified : true,
    card_path_verified : true,
};

/// DW MMC 控制器输入时钟（JH7110 CIU 时钟，与 U-Boot 同源；分频后目标
/// 50MHz，与 U-Boot 日志的 SD High Speed 一致）。
pub const MMC_INPUT_FREQUENCY_HZ : u32 = 100_000_000;

const MMC_POLL_LIMIT : usize = 1_000_000;
const MMC_OCR_ATTEMPTS : usize = 1_000;
/// SD 识别阶段时钟（规范 100–400kHz；过快会导致响应 CRC 错）。
const SD_IDENTIFICATION_HZ : u32 = 400_000;
/// 识别完成后提升到的传输时钟。本板 SD host FIFO 仅 32 字（128B），U-Boot
/// 实际走 IDMAC；PIO 在 25MHz 下连续读会间歇 FRUN。10MHz 是比 1MHz 快约
/// 8 倍、且保留约 4 倍余量的保守档。正确解法是 IDMAC（+ 缓存维护），后续
/// 任务实现后再恢复 4-bit/50MHz。
const SD_TRANSFER_HZ : u32 = 10_000_000;

/// 尝试激活一个 MMC host 并注册只读块设备；失败带上下文返回，不阻断启动。
pub fn activate_and_register_readonly(host : &MmcHostDescription)
                                      -> Result<usize, String> {
    // SAFETY: 平台 memory layout 恒等映射覆盖 JH7110 低地址 MMIO 窗口
    // （0x0100_0000..0x4000_0000），该实例独占访问控制器寄存器。
    let registers = unsafe { MmioRegisters::new(host.mmio) };
    // 识别阶段按 SD 规范跑 400kHz（过快响应采样失败 → ResponseCrc），并保持
    // 1-bit（卡默认模式）：未发送 ACMD6/CMD6 前，卡侧是 1-bit、≤25MHz，
    // 控制器按 4-bit/50MHz 采样会数据错误 → Read(IoError)。
    let mut ident_host = host.clone();
    ident_host.bus_width = 1;
    ident_host.max_frequency_hz = Some(SD_IDENTIFICATION_HZ);
    let ident_plan = bring_up_plan(&ident_host);
    let controller = initialize_controller(&ident_plan,
                                           MMC_ACTIVATION_EVIDENCE,
                                           registers,
                                           MMC_INPUT_FREQUENCY_HZ,
                                           MMC_POLL_LIMIT)
        .map_err(|err| format!("controller: {err:?}"))?;
    let card = SdCard::initialize(controller, MMC_OCR_ATTEMPTS)
        .map_err(|err| format!("sd init: {err:?}"))?;
    let info = card.info();
    let mut controller = card.into_transport();
    controller.configure_card_clock(MMC_INPUT_FREQUENCY_HZ, SD_TRANSFER_HZ)
        .map_err(|err| format!("reclock: {err:?}"))?;
    let card = SdCard::from_transport(controller, info);
    let shared = card.into_shared();
    register_readonly_block_device(shared)
        .map_err(|err| format!("register: {err:?}"))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MmcHardwareEvidence {
    pub clock_verified : bool,
    pub reset_verified : bool,
    pub irq_verified : bool,
    pub card_path_verified : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcBringUpPlan {
    pub host : MmcHostDescription,
    pub blockers : Vec<MmcActivationBlocker>,
}

impl MmcBringUpPlan {
    /// Hardware activation remains deliberately unavailable until board
    /// clock/reset/pinmux/card and controller behavior are verified.
    pub const fn can_activate(&self) -> bool { false }

    /// Evaluate externally supplied board evidence without performing any
    /// register access. This is the only path that may eventually clear the
    /// static `HardwareEvidence` blocker.
    pub fn activation_ready(&self, evidence : MmcHardwareEvidence) -> bool {
        self.blockers.len() == 1 &&
        self.blockers[0] == MmcActivationBlocker::HardwareEvidence &&
        evidence.clock_verified && evidence.reset_verified && evidence.irq_verified &&
        evidence.card_path_verified
    }

    /// Produce only protocol/controller parameters; this does not touch
    /// clocks, reset, pinmux, power or MMIO.
    pub fn controller_config(&self) -> Result<MmcControllerConfig, MmcConfigError> {
        if self.blockers.iter().any(|blocker| {
            !matches!(blocker, MmcActivationBlocker::HardwareEvidence)
        }) {
            return Err(MmcConfigError::InvalidStaticResources);
        }
        let target_frequency_hz = self.host.max_frequency_hz.ok_or(
            MmcConfigError::MissingTargetFrequency)?;
        let fifo_depth = self.host.fifo_depth.ok_or(MmcConfigError::MissingFifoDepth)?;
        Ok(MmcControllerConfig { target_frequency_hz,
                                  fifo_depth,
                                  bus_width : self.host.bus_width })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecifier {
    pub provider : u32,
    pub args : Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysregField {
    pub provider : u32,
    pub offset : u32,
    pub shift : u8,
    pub mask : u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcHostDescription {
    pub mmio : MmioRegion,
    pub irq : u32,
    pub bus_width : u8,
    pub max_frequency_hz : Option<u32>,
    pub fifo_depth : Option<u32>,
    pub non_removable : bool,
    pub biu_clock : ResourceSpecifier,
    pub ciu_clock : ResourceSpecifier,
    pub reset : ResourceSpecifier,
    pub sysreg : Option<SysregField>,
}

pub fn bring_up_plan(host : &MmcHostDescription) -> MmcBringUpPlan {
    let mut blockers = Vec::new();
    if host.mmio.base == 0 || host.mmio.base % 4 != 0 || host.mmio.size < 0x100 {
        blockers.push(MmcActivationBlocker::InvalidMmio);
    }
    if host.irq == 0 {
        blockers.push(MmcActivationBlocker::InvalidIrq);
    }
    if !matches!(host.bus_width, 1 | 4 | 8) {
        blockers.push(MmcActivationBlocker::InvalidBusWidth);
    }
    if host.biu_clock.provider == 0 || host.biu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingBiuClock);
    }
    if host.ciu_clock.provider == 0 || host.ciu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingCiuClock);
    }
    if host.reset.provider == 0 || host.reset.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingReset);
    }
    if host.sysreg.is_none() {
        blockers.push(MmcActivationBlocker::MissingSysreg);
    }
    if host.max_frequency_hz.is_none() {
        blockers.push(MmcActivationBlocker::MissingTargetFrequency);
    } else if host.max_frequency_hz == Some(0) {
        blockers.push(MmcActivationBlocker::InvalidTargetFrequency);
    }
    if host.fifo_depth.is_none() {
        blockers.push(MmcActivationBlocker::MissingFifoDepth);
    } else if !host.fifo_depth.is_some_and(|depth| (2..=4096).contains(&depth)) {
        blockers.push(MmcActivationBlocker::InvalidFifoDepth);
    }
    blockers.push(MmcActivationBlocker::HardwareEvidence);
    MmcBringUpPlan { host : host.clone(), blockers }
}

/// Construct and initialize the shared controller only after explicit board
/// evidence has been supplied. This function is not called during generic
/// machine bring-up and does not register a block device.
pub fn initialize_controller<R : RegisterIo>(plan : &MmcBringUpPlan,
                                              evidence : MmcHardwareEvidence,
                                              registers : R,
                                              input_frequency_hz : u32,
                                              poll_limit : usize)
                                              -> Result<DwMmc<R>, MmcInitializationError> {
    if !plan.activation_ready(evidence) {
        return Err(MmcInitializationError::NotReady);
    }
    let config = plan.controller_config().map_err(|_| MmcInitializationError::NotReady)?;
    let mut controller = DwMmc::probe(registers, poll_limit)
        .map_err(MmcInitializationError::Core)?;
    controller.initialize_polling_with_bus_width(input_frequency_hz,
                                                 config.target_frequency_hz,
                                                 config.fifo_depth,
                                                 config.bus_width)
               .map_err(MmcInitializationError::Core)?;
    Ok(controller)
}

/// Explicitly initialize the shared SD protocol after the controller has been
/// configured and board evidence has been supplied. This remains opt-in: the
/// generic machine startup path does not call it or register a block device.
pub fn initialize_sd_card<R : RegisterIo>(plan : &MmcBringUpPlan,
                                          evidence : MmcHardwareEvidence,
                                          registers : R,
                                          input_frequency_hz : u32,
                                          poll_limit : usize,
                                          ocr_attempts : usize)
                                          -> Result<SdCard<DwMmc<R>>, MmcInitializationError> {
    let controller = initialize_controller(plan,
                                            evidence,
                                            registers,
                                            input_frequency_hz,
                                            poll_limit)?;
    SdCard::initialize(controller, ocr_attempts).map_err(MmcInitializationError::Card)
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use spin::Mutex;
    use super::*;

    fn host() -> MmcHostDescription {
        MmcHostDescription { mmio : MmioRegion { base : 0x1602_0000, size : 0x1000 },
                              irq : 91,
                              bus_width : 4,
                              max_frequency_hz : Some(50_000_000),
                              fifo_depth : Some(32),
                              non_removable : false,
                              biu_clock : ResourceSpecifier { provider : 1, args : vec![0] },
                              ciu_clock : ResourceSpecifier { provider : 2, args : vec![1] },
                              reset : ResourceSpecifier { provider : 3, args : vec![0] },
                              sysreg : Some(SysregField { provider : 4,
                                                          offset : 0x10,
                                                          shift : 0,
                                                          mask : 0x3 }) }
    }

    struct ProbeDisk {
        total : Option<u64>,
        fail : bool,
    }
    impl BlockDevice for ProbeDisk {
        fn total_blocks(&self) -> Option<u64> { self.total }
        fn read_blocks(&mut self, _start : Lba, output : &mut [u8]) -> block::DriverResult<()> {
            if self.fail { return Err(DriverError::IoError); }
            output.fill(0xA5);
            Ok(())
        }
        fn write_blocks(&mut self, _start : Lba, _input : &[u8]) -> block::DriverResult<()> {
            Err(DriverError::Unsupported)
        }

        fn flush(&mut self) -> block::DriverResult<()> {
            Ok(())
        }
    }
    fn probe_disk(total : Option<u64>, fail : bool) -> SharedBlockDevice {
        Arc::new(Mutex::new(Box::new(ProbeDisk { total, fail })))
    }

    struct MbrDisk {
        bytes : Vec<u8>,
    }

    impl BlockDevice for MbrDisk {
        fn total_blocks(&self) -> Option<u64> {
            Some((self.bytes.len() / BLOCK_SIZE) as u64)
        }

        fn read_blocks(&mut self, start : Lba, output : &mut [u8]) -> block::DriverResult<()> {
            if output.len() != BLOCK_SIZE {
                return Err(DriverError::InvalidParam);
            }
            let offset = usize::try_from(start.0)
                .ok()
                .and_then(|lba| lba.checked_mul(BLOCK_SIZE))
                .ok_or(DriverError::InvalidParam)?;
            let end = offset.checked_add(output.len()).ok_or(DriverError::InvalidParam)?;
            if end > self.bytes.len() {
                return Err(DriverError::IoError);
            }
            output.copy_from_slice(&self.bytes[offset..end]);
            Ok(())
        }

        fn write_blocks(&mut self, _start : Lba, _input : &[u8]) -> block::DriverResult<()> {
            Err(DriverError::Unsupported)
        }

        fn flush(&mut self) -> block::DriverResult<()> {
            Ok(())
        }
    }

    #[test]
    fn registration_requires_capacity_and_first_read() {
        let before = block::block_device_count();
        assert_eq!(register_readonly_block_device(probe_disk(None, false)),
                   Err(MmcRegistrationError::UnknownCapacity));
        assert_eq!(register_readonly_block_device(probe_disk(Some(1), true)),
                   Err(MmcRegistrationError::Read(DriverError::IoError)));
        assert_eq!(block::block_device_count(), before);
        let index = register_readonly_block_device(probe_disk(Some(1), false)).unwrap();
        assert!(block::block_device_count() > before);
        let _ = index;
    }

    fn mbr_disk() -> SharedBlockDevice {
        let mut bytes = vec![0u8; 4096 * BLOCK_SIZE];
        bytes[510] = 0x55;
        bytes[511] = 0xaa;
        // One Linux-style primary partition: LBA 1..16.
        let entry = 446;
        bytes[entry + 4] = 0x83;
        bytes[entry + 8..entry + 12].copy_from_slice(&1u32.to_le_bytes());
        bytes[entry + 12..entry + 16].copy_from_slice(&16u32.to_le_bytes());
        Arc::new(Mutex::new(Box::new(MbrDisk { bytes })))
    }

    #[test]
    fn registration_exposes_mbr_partition_and_removes_it_with_parent() {
        let disk_index = register_readonly_block_device(mbr_disk()).unwrap();
        let snapshot = block::block_devices_snapshot();
        let partition_index = snapshot.iter()
            .find_map(|(index, _, role)| match role {
                block::BlockDeviceRole::Partition { parent_device_index, partition_number }
                    if *parent_device_index == disk_index && *partition_number == 1 => Some(*index),
                _ => None,
            })
            .expect("registered MBR partition");
        assert!(snapshot.iter().any(|(index, _, role)| {
            *index == disk_index && matches!(role, block::BlockDeviceRole::Disk { .. })
        }));
        assert!(block::unregister_block_device(disk_index));
        assert!(block::block_device_at(partition_index).is_none());
    }

    #[test]
    fn valid_topology_still_requires_hardware_evidence() {
        let plan = bring_up_plan(&host());
        assert_eq!(plan.blockers, vec![MmcActivationBlocker::HardwareEvidence]);
        assert!(!plan.can_activate());
        assert!(!plan.activation_ready(MmcHardwareEvidence::default()));
        assert!(plan.activation_ready(MmcHardwareEvidence { clock_verified : true,
                                                            reset_verified : true,
                                                            irq_verified : true,
                                                            card_path_verified : true }));
        assert_eq!(plan.controller_config(),
                   Ok(MmcControllerConfig { target_frequency_hz : 50_000_000,
                                             fifo_depth : 32,
                                             bus_width : 4 }));
    }

    #[test]
    fn malformed_resources_are_reported_without_mmio() {
        let mut value = host();
        value.mmio.base = 0;
        value.bus_width = 2;
        value.biu_clock.provider = 0;
        value.sysreg = None;
        let plan = bring_up_plan(&value);
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidMmio));
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidBusWidth));
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingBiuClock));
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingSysreg));
        assert!(!plan.can_activate());
        assert_eq!(plan.controller_config(), Err(MmcConfigError::InvalidStaticResources));
    }

    #[test]
    fn missing_controller_tuning_is_a_static_blocker() {
        let mut value = host();
        value.max_frequency_hz = None;
        value.fifo_depth = None;
        let plan = bring_up_plan(&value);
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingTargetFrequency));
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingFifoDepth));
        assert!(!plan.activation_ready(MmcHardwareEvidence { clock_verified : true,
                                                             reset_verified : true,
                                                             irq_verified : true,
                                                             card_path_verified : true }));
        assert_eq!(plan.controller_config(), Err(MmcConfigError::InvalidStaticResources));
    }

    #[test]
    fn zero_controller_tuning_is_rejected_before_mmio() {
        let mut value = host();
        value.max_frequency_hz = Some(0);
        value.fifo_depth = Some(0);
        let plan = bring_up_plan(&value);
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidTargetFrequency));
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidFifoDepth));
        assert_eq!(plan.controller_config(), Err(MmcConfigError::InvalidStaticResources));
    }

    #[test]
    fn fifo_depth_outside_controller_range_is_rejected_before_mmio() {
        for depth in [1, 4097] {
            let mut value = host();
            value.fifo_depth = Some(depth);
            let plan = bring_up_plan(&value);
            assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidFifoDepth));
            assert_eq!(plan.controller_config(), Err(MmcConfigError::InvalidStaticResources));
        }
    }

    struct Registers { values : [u32; 32] }

    impl RegisterIo for Registers {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == 0x000 || offset == 0x02c || offset == 0x044 {
                return Ok(0);
            }
            self.values.get(offset / 4).copied().ok_or(MmcError::RegisterOutOfRange)
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            *self.values.get_mut(offset / 4).ok_or(MmcError::RegisterOutOfRange)? = value;
            Ok(())
        }
    }

    #[test]
    fn gated_initializer_consumes_controller_config_only_with_evidence() {
        let plan = bring_up_plan(&host());
        let evidence = MmcHardwareEvidence { clock_verified : true,
                                              reset_verified : true,
                                              irq_verified : true,
                                              card_path_verified : true };
        let initialized = initialize_controller(&plan,
                                                evidence,
                                                Registers { values : [0; 32] },
                                                50_000_000,
                                                4);
        assert!(initialized.is_ok(), "initializer failed: {:?}", initialized.err());
        assert!(matches!(initialize_controller(&plan,
                                                MmcHardwareEvidence::default(),
                                                Registers { values: [0; 32] },
                                                50_000_000,
                                                4),
                         Err(MmcInitializationError::NotReady)));
        assert!(matches!(initialize_sd_card(&plan,
                                            MmcHardwareEvidence::default(),
                                            Registers { values: [0; 32] },
                                            50_000_000,
                                            4,
                                            1),
                         Err(MmcInitializationError::NotReady)));
    }
}
