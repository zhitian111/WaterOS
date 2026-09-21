//! Loongson 2K1000 AHCI/SATA 块设备（轮询 DMA，无中断）。
//!
//! 复用实板验证过的 `simple-ahci` 与 robigalia `pci`；本 crate 提供
//! WaterOS HAL、PCI 扫描与 [`BlockDevice`] 适配。旧 bring-up 后端暂时保留作诊断对照。

#![no_std]
extern crate alloc;

#[cfg(feature = "dma")]
use alloc::vec::Vec;
use block::BLOCK_SIZE;
#[cfg(feature = "dma")]
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering, compiler_fence};
#[cfg(feature = "dma")]
use block::{BlockDevice, DriverError, DriverResult, Lba};
#[cfg(feature = "dma")]
use isomorphic_drivers::provider::Provider;

/// Loongson 2K1000 PCIe 配置空间（ECAM/CAM）内核 VA 别名。
///
/// 物理 ECAM 基址为 `0xfe_0000_0000`，但当前 LoongArch 三级页表只能表示
/// 低 39 位地址；内核页表初始化时把它映射到 `0x40_0000_0000`，此处使用同一
/// 内核 VA，让 PCI 扫描代码无需感知高物理地址。
pub const PCI_CONFIG_BASE : usize = 0x40_0000_0000;
/// ATA 大容量存储设备类（class=0x01）下的 SATA 控制器子类（subclass=0x06）。
pub const PCI_CLASS_MASS_STORAGE : u8 = 0x01;
pub const PCI_SUBCLASS_SATA : u8 = 0x06;
/// 2K1000 片上 SATA 控制器固定为 bus 0 / device 8 / function 0。
const SATA_CONFIG_OFFSET : usize = 8 << 11;
/// 2K1000LA 通用配置寄存器0；bit 8 为 `sata_sel`。
const LOONGSON2K1000_GENERAL_CONFIG0 : usize = 0x1FE0_0420;
/// 2K1000LA SATA PHY 芯片级配置寄存器（用户手册 5.7 节）。
const LOONGSON2K1000_SATA_CONFIG : usize = 0x1FE0_0450;
/// SoC clock-divider configuration; bits 14:12 select the SATA divider.
const LOONGSON2K1000_FREQSCALE_CONFIG : usize = 0x1FE0_04D0;
/// PM controller dynamic-power-management registers (manual chapter 29).
const LOONGSON2K1000_DPM_CONFIG : usize = 0x1FE2_7400;
const LOONGSON2K1000_DPM_STATUS : usize = 0x1FE2_7404;
const LOONGSON2K1000_DPM_CONTROL : usize = 0x1FE2_7408;
const SATA_CONFIG_PHY_POWER_DOWN : u64 = 1 << 63;
/// SATA DMA request cache-coherency attribute (manual section 5.7, bit 10).
const SATA_CONFIG_DMA_COHERENT : u64 = 1 << 10;
/// LANE0/PHY 软件复位均为低有效。
const SATA_CONFIG_LANE0_RESET_N : u64 = 1 << 3;
const SATA_CONFIG_PHY_RESET_N : u64 = 1 << 2;
const SATA_CONFIG_REF_CLOCK_ENABLE : u64 = 1 << 0;
#[cfg(feature = "dma")]
const SATA_PHY_RESET_HOLD_MS : usize = 10;
#[cfg(feature = "dma")]
const SATA_PHY_SETTLE_MS : usize = 100;

const fn prepared_soc_sata_config(config : u64) -> u64 {
    let required =
        SATA_CONFIG_LANE0_RESET_N | SATA_CONFIG_PHY_RESET_N | SATA_CONFIG_REF_CLOCK_ENABLE;
    // The Linux device tree marks the 2K1000 PCIe bus dma-coherent, and PMON
    // leaves this attribute enabled. Preserve it while repairing only the
    // PHY power/reset prerequisites.
    (config | required) & !SATA_CONFIG_PHY_POWER_DOWN
}

#[cfg(any(feature = "dma", test))]
/// DMW0: uncached, direct physical-memory view on LoongArch.
///
/// The AHCI controller writes command status, RFIS and data without any CPU
/// cache maintenance primitive available in this port. Keeping both command
/// descriptors and payloads in DMW0 makes the A/B test independent of cache
/// flush/invalidate support while the device still receives the low PA.
const LOONGARCH64_UNCACHED_WINDOW_BASE : usize = 0x8000_0000_0000_0000;
#[cfg(any(feature = "dma", test))]
const LOONGARCH64_CACHED_WINDOW_BASE : usize = 0x9000_0000_0000_0000;
#[cfg(any(feature = "dma", test))]
const LOONGARCH64_PHYS_ADDR_MASK : usize = 0x0000_FFFF_FFFF_FFFF;
#[cfg(any(feature = "dma", test))]
const LOONGARCH64_WINDOW_BASE_MASK : usize = 0xFFFF_0000_0000_0000;
/// Diagnostic command-list page in the first low-memory DRAM bank.
///
/// The AHCI core performs its control-page allocation before its data-page
/// allocation.  Keeping only that first page at a known low DMA address makes
/// the command-fetch A/B independent of PRDT/data DMA.
#[cfg(feature = "dma")]
const AHCI_LOW_CONTROL_DMA_PA : usize = 0x0B00_0000;
#[cfg(feature = "dma")]
static AHCI_LOW_CONTROL_DMA_CLAIMED : AtomicBool = AtomicBool::new(false);
const LOONGSON2K1000_XBAR_WIN4_BASE : usize = 0x1FE0_2400;
const LOONGSON2K1000_XBAR_WIN4_MASK : usize = 0x1FE0_2440;
const LOONGSON2K1000_XBAR_WIN4_MMAP : usize = 0x1FE0_2480;
const LOONGSON2K1000_XBAR_WIN5_BASE : usize = 0x1FE0_2500;
const LOONGSON2K1000_XBAR_WIN5_MASK : usize = 0x1FE0_2540;
const LOONGSON2K1000_XBAR_WIN5_MMAP : usize = 0x1FE0_2580;
/// 2K1000LA 芯片只实现一个 SATA 端口；部分固件未填写 AHCI PI。
#[cfg(feature = "dma")]
const LOONGSON2K1000_PORT_MAP : u32 = 1;

/// 纯函数：判定 class/subclass 是否为 SATA 大容量存储控制器（便于 host 单测）。
pub const fn is_sata_mass_storage_class(class_code : u8, subclass : u8) -> bool {
    class_code == PCI_CLASS_MASS_STORAGE && subclass == PCI_SUBCLASS_SATA
}

/// buf 字节数 → 512B 扇区数（纯函数，便于 host 单测）。
pub const fn sector_count_for(byte_len : usize) -> Option<usize> {
    if byte_len % BLOCK_SIZE != 0 {
        None
    } else {
        Some(byte_len / BLOCK_SIZE)
    }
}

/// DMA 请求字节数 → 所需页数（按页向上取整；host 单测覆盖对齐/溢出）。
pub const fn dma_pages_for(byte_len : usize) -> Option<usize> {
    const PAGE : usize = 4096;
    if byte_len == 0 {
        return None;
    }
    match byte_len.checked_add(PAGE - 1) {
        Some(value) => Some(value / PAGE),
        None => None,
    }
}

#[cfg(any(feature = "dma", test))]
#[inline]
const fn dma_phys_to_kernel_va(pa : usize) -> Option<usize> {
    if pa & !LOONGARCH64_PHYS_ADDR_MASK != 0 {
        None
    } else {
        Some(LOONGARCH64_UNCACHED_WINDOW_BASE | pa)
    }
}

#[cfg(any(feature = "dma", test))]
#[inline]
const fn dma_kernel_va_to_phys(va : usize) -> Option<usize> {
    if va & LOONGARCH64_WINDOW_BASE_MASK != LOONGARCH64_UNCACHED_WINDOW_BASE {
        None
    } else {
        Some(va & LOONGARCH64_PHYS_ADDR_MASK)
    }
}

fn prepare_soc_sata_phy() {
    let general_config0 =
        unsafe { core::ptr::read_volatile(LOONGSON2K1000_GENERAL_CONFIG0 as *const u64) };
    log::info!("[ahci] SoC general_config0={:#018x} sata_sel={}",
               general_config0,
               (general_config0 >> 8) & 1);

    let freqscale =
        unsafe { core::ptr::read_volatile(LOONGSON2K1000_FREQSCALE_CONFIG as *const u64) };
    let dpm_config = unsafe { core::ptr::read_volatile(LOONGSON2K1000_DPM_CONFIG as *const u32) };
    let dpm_status = unsafe { core::ptr::read_volatile(LOONGSON2K1000_DPM_STATUS as *const u32) };
    let dpm_control = unsafe { core::ptr::read_volatile(LOONGSON2K1000_DPM_CONTROL as *const u32) };
    log::info!("[ahci] SoC freqscale={:#018x} sata_div={} DPM cfg={:#010x} sts={:#010x} \
                cnt={:#010x} sata_en={} running={} state={} target={}",
               freqscale,
               (freqscale >> 12) & 0x7,
               dpm_config,
               dpm_status,
               dpm_control,
               (dpm_config >> 4) & 1,
               (dpm_status >> 28) & 1,
               (dpm_status >> 8) & 0x3,
               (dpm_control >> 8) & 0x3);

    let sata_config_ptr = LOONGSON2K1000_SATA_CONFIG as *mut u64;
    let sata_config = unsafe { core::ptr::read_volatile(sata_config_ptr) };
    log::info!("[ahci] SoC sata_config={:#018x} power_down={} dma_coherent={} lane0_reset_n={} \
                phy_reset_n={} ref_external={} ref_clock_enable={}",
               sata_config,
               (sata_config >> 63) & 1,
               (sata_config & SATA_CONFIG_DMA_COHERENT != 0) as u8,
               (sata_config >> 3) & 1,
               (sata_config >> 2) & 1,
               (sata_config >> 1) & 1,
               sata_config & 1);

    // Firmware normally leaves the PHY powered, clocked and out of reset.  Do
    // not disturb analogue tuning or board-selected reference-clock fields;
    // repair only the documented power/reset/clock prerequisites when needed.
    let prepared = prepared_soc_sata_config(sata_config);
    if prepared != sata_config {
        unsafe {
            core::ptr::write_volatile(sata_config_ptr, prepared);
        }
        let readback = unsafe { core::ptr::read_volatile(sata_config_ptr) };
        log::warn!("[ahci] prepared SoC SATA PHY prerequisites requested={:#018x} \
                    readback={:#018x} dma_coherent={}",
                   prepared,
                   readback,
                   (readback & SATA_CONFIG_DMA_COHERENT != 0) as u8);
    }

    // The DWC core receives COMINIT but does not complete OOB negotiation on
    // firmware hand-off. Pulse the documented active-low PHY and lane resets
    // while preserving every analogue tuning and reference-clock selection
    // bit. The subsequent HBA reset rebuilds the controller-side state.
    let reset_mask = SATA_CONFIG_LANE0_RESET_N | SATA_CONFIG_PHY_RESET_N;
    let asserted = prepared & !reset_mask;
    unsafe {
        core::ptr::write_volatile(sata_config_ptr, asserted);
    }
    let asserted_readback = unsafe { core::ptr::read_volatile(sata_config_ptr) };
    log::info!("[ahci] SoC SATA PHY reset asserted requested={:#018x} readback={:#018x}",
               asserted,
               asserted_readback);
    #[cfg(feature = "dma")]
    FrameProvider::delay_ms(SATA_PHY_RESET_HOLD_MS);

    unsafe {
        core::ptr::write_volatile(sata_config_ptr, prepared);
    }
    let released_readback = unsafe { core::ptr::read_volatile(sata_config_ptr) };
    log::info!("[ahci] SoC SATA PHY reset released requested={:#018x} readback={:#018x}",
               prepared,
               released_readback);
    #[cfg(feature = "dma")]
    FrameProvider::delay_ms(SATA_PHY_SETTLE_MS);
}

#[cfg(feature = "dma")]
fn log_sata_dma_windows() {
    let read = |address : usize| unsafe { core::ptr::read_volatile(address as *const u64) };
    let mut win4_enabled = 0u8;
    let mut win5_enabled = 0u8;
    for index in 0..8usize {
        let base = read(LOONGSON2K1000_XBAR_WIN4_BASE + index * 8);
        let mask = read(LOONGSON2K1000_XBAR_WIN4_MASK + index * 8);
        let mmap = read(LOONGSON2K1000_XBAR_WIN4_MMAP + index * 8);
        if mmap & (1 << 7) != 0 {
            win4_enabled |= 1 << index;
        }
        log::info!("[ahci] IODMA WIN4[{}] base={:#018x} mask={:#018x} mmap={:#018x}",
                   index,
                   base,
                   mask,
                   mmap);

        let base = read(LOONGSON2K1000_XBAR_WIN5_BASE + index * 8);
        let mask = read(LOONGSON2K1000_XBAR_WIN5_MASK + index * 8);
        let mmap = read(LOONGSON2K1000_XBAR_WIN5_MMAP + index * 8);
        if mmap & (1 << 7) != 0 {
            win5_enabled |= 1 << index;
        }
        log::info!("[ahci] IODMA WIN5[{}] base={:#018x} mask={:#018x} mmap={:#018x}",
                   index,
                   base,
                   mask,
                   mmap);
    }
    log::info!("[ahci] IODMA enabled windows WIN4={:#04x} WIN5={:#04x} (integrated SATA uses \
                WIN4)",
               win4_enabled,
               win5_enabled);
}

#[cfg(feature = "dma")]
fn log_simple_ahci_dma_policy() {
    // Match the StarryOS adapter: descriptors come from the coherent kernel
    // heap, while MMIO itself uses the uncached DMW0 alias.
    log::info!("[ahci] simple-ahci coherent heap DMA; preserving firmware IODMA windows");
}

fn log_fixed_sata_config() {
    let base = PCI_CONFIG_BASE + SATA_CONFIG_OFFSET;
    let read = |offset : usize| unsafe { core::ptr::read_volatile((base + offset) as *const u32) };
    log::info!("[ahci] 00:08.0 cfg id={:#010x} command={:#010x} class={:#010x} header={:#010x} \
                bar0={:#010x}:{:#010x}",
               read(0x00),
               read(0x04),
               read(0x08),
               read(0x0C),
               read(0x14),
               read(0x10));
}

#[cfg(feature = "dma")]
fn log_hba_state(base : usize, phase : &str) {
    let read = |offset : usize| unsafe { core::ptr::read_volatile((base + offset) as *const u32) };
    let cap = read(0x00);
    let ghc = read(0x04);
    let interrupt_status = read(0x08);
    let ports_implemented = read(0x0C);
    let version = read(0x10);
    log::info!("[ahci] {} HBA cap={:#010x} ghc={:#010x} is={:#010x} pi={:#010x} vs={:#010x}",
               phase,
               cap,
               ghc,
               interrupt_status,
               ports_implemented,
               version);
    log::info!("[ahci] {} HBA cap2={:#010x} bohc={:#010x} oobr={:#010x} timer1ms={:#010x} \
                gparam1={:#010x} gparam2={:#010x} pparam={:#010x} test={:#010x} \
                versionr={:#010x} idr={:#010x}",
               phase,
               read(0x24),
               read(0x28),
               read(0xBC),
               read(0xE0),
               read(0xE8),
               read(0xEC),
               read(0xF0),
               read(0xF4),
               read(0xF8),
               read(0xFC));

    let port_count = ((cap & 0x1F) + 1).min(32);
    let diagnostic_port_map = if ports_implemented == 0 {
        if port_count == 32 {
            u32::MAX
        } else {
            (1u32 << port_count) - 1
        }
    } else {
        ports_implemented
    };
    for port in 0..port_count {
        if diagnostic_port_map & (1 << port) == 0 {
            continue;
        }
        let port_base = 0x100 + port as usize * 0x80;
        let ssts = read(port_base + 0x28);
        let tfd = read(port_base + 0x20);
        log::info!("[ahci] {} port{} cmd={:#010x} tfd={:#010x}(sts={:#04x} err={:#04x}) \
                    sig={:#010x} ssts={:#010x}(det={} spd={} ipm={}) sctl={:#010x} serr={:#010x} \
                    is={:#010x} ie={:#010x} sact={:#010x} ci={:#010x} sntf={:#010x} fbs={:#010x} \
                    dmacr={:#010x} phycr={:#010x} physr={:#010x}",
                   phase,
                   port,
                   read(port_base + 0x18),
                   tfd,
                   tfd & 0xFF,
                   (tfd >> 8) & 0xFF,
                   read(port_base + 0x24),
                   ssts,
                   ssts & 0xF,
                   (ssts >> 4) & 0xF,
                   (ssts >> 8) & 0xF,
                   read(port_base + 0x2C),
                   read(port_base + 0x30),
                   read(port_base + 0x10),
                   read(port_base + 0x14),
                   read(port_base + 0x34),
                   read(port_base + 0x38),
                   read(port_base + 0x3C),
                   read(port_base + 0x40),
                   read(port_base + 0x70),
                   read(port_base + 0x78),
                   read(port_base + 0x7C));
    }
}

/// 扫描 PCI 配置空间，返回首个 SATA 控制器的 BAR0（基址、长度）。
///
/// 该路径访问真实 MMIO，不做 host 单测；class 匹配逻辑由
/// [`is_sata_mass_storage_class`] 单独测试。
pub fn find_ahci_bar() -> Option<(usize, usize)> {
    log_fixed_sata_config();
    #[cfg(feature = "dma")]
    log_sata_dma_windows();
    #[cfg(feature = "dma")]
    log_simple_ahci_dma_policy();
    for dev in unsafe {
        pci::scan_bus(&UnusedPort,
                      pci::CSpaceAccessMethod::MemoryMapped,
                      PCI_CONFIG_BASE)
    } {
        if !is_sata_mass_storage_class(dev.id.class, dev.id.subclass) {
            continue;
        }
        if let Some(pci::BAR::Memory(pa, len, _, _)) = dev.bars[0] {
            if pa != 0 {
                log::info!("[ahci] found SATA controller bar0={:#x} len={:#x}",
                           pa,
                           len);
                return Some((pa as usize, len as usize));
            }
        }
    }
    None
}

/// 在 2K1000 PCIe ECAM 上枚举 AHCI 控制器并注册为块设备（需要 `dma` feature）。
#[cfg(feature = "dma")]
pub fn init() -> DriverResult<usize> {
    use alloc::{boxed::Box, sync::Arc};
    use block::{register_block_device, SharedBlockDevice};
    use spin::Mutex;

    let (base, size) = find_ahci_bar().ok_or(DriverError::NotFound)?;
    let sata = SataBlock::new(base, size)?;
    let shared : SharedBlockDevice = Arc::new(Mutex::new(Box::new(sata)));
    Ok(register_block_device(shared))
}

/// 为 AHCI 提供来自内核 RAM 的物理连续 DMA 页。
#[cfg(feature = "dma")]
pub struct FrameProvider;

#[cfg(feature = "dma")]
impl Provider for FrameProvider {
    const PAGE_SIZE : usize = 4096;

    fn alloc_dma(size : usize) -> (usize, usize) {
        let Some(pages) = dma_pages_for(size) else {
            return (0, 0);
        };
        let Some(bytes) = pages.checked_mul(Self::PAGE_SIZE) else {
            return (0, 0);
        };

        if pages == 1 &&
           AHCI_LOW_CONTROL_DMA_CLAIMED.compare_exchange(false,
                                                         true,
                                                         Ordering::AcqRel,
                                                         Ordering::Acquire)
                                       .is_ok()
        {
            let Some(vaddr) = dma_phys_to_kernel_va(AHCI_LOW_CONTROL_DMA_PA) else {
                AHCI_LOW_CONTROL_DMA_CLAIMED.store(false, Ordering::Release);
                return (0, 0);
            };
            unsafe {
                core::ptr::write_bytes(vaddr as *mut u8, 0, bytes);
            }
            log::info!("[ahci] fixed low control DMA allocation va={:#x} pa={:#x} bytes={:#x}",
                       vaddr,
                       AHCI_LOW_CONTROL_DMA_PA,
                       bytes);
            return (vaddr, AHCI_LOW_CONTROL_DMA_PA);
        }

        let mut ppns = Vec::with_capacity(pages);
        for _ in 0..pages {
            match frame_alloctor::frame_alloc_result() {
                Ok(ppn) => ppns.push(ppn),
                Err(_) => {
                    release_dma_frames(&ppns);
                    return (0, 0);
                }
            }
        }
        // The stack frame allocator returns descending PPNs. AHCI requires
        // each allocation to be physically contiguous.
        if !ppns.windows(2)
                .all(|pair| pair[0].0 == pair[1].0 + 1)
        {
            release_dma_frames(&ppns);
            return (0, 0);
        }
        let Some(base_pa) = ppns[pages - 1].0
                                           .checked_mul(Self::PAGE_SIZE)
        else {
            release_dma_frames(&ppns);
            return (0, 0);
        };
        let Some(vaddr) = dma_phys_to_kernel_va(base_pa) else {
            release_dma_frames(&ppns);
            return (0, 0);
        };
        unsafe {
            core::ptr::write_bytes(vaddr as *mut u8, 0, bytes);
        }
        log::info!("[ahci] frame DMA allocation va={:#x} pa={:#x} bytes={:#x}",
                   vaddr,
                   base_pa,
                   bytes);
        (vaddr, base_pa)
    }

    fn dealloc_dma(vaddr : usize, size : usize) {
        let Some(pages) = dma_pages_for(size) else {
            return;
        };
        let Some(base_pa) = dma_kernel_va_to_phys(vaddr) else {
            return;
        };
        if base_pa % Self::PAGE_SIZE != 0 {
            return;
        }
        if base_pa == AHCI_LOW_CONTROL_DMA_PA {
            AHCI_LOW_CONTROL_DMA_CLAIMED.store(false, Ordering::Release);
            return;
        }
        for index in 0..pages {
            let ppn = mm_api::addr::PhysPageNum(base_pa / Self::PAGE_SIZE + index);
            let _ = frame_alloctor::frame_dealloc_result(ppn);
        }
    }

    fn delay_ms(milliseconds : usize) {
        let Ok(start) = platform::timer::now_tick() else {
            for _ in 0..milliseconds.saturating_mul(100_000) {
                core::hint::spin_loop();
            }
            return;
        };
        let Ok(frequency) = platform::timer::tick_hz() else {
            for _ in 0..milliseconds.saturating_mul(100_000) {
                core::hint::spin_loop();
            }
            return;
        };
        let delta = frequency.0
                             .saturating_mul(milliseconds as u64)
                             .div_ceil(1_000);
        let deadline = start.0
                            .saturating_add(delta);
        while platform::timer::now_tick().is_ok_and(|now| now.0 < deadline) {
            core::hint::spin_loop();
        }
    }
}

#[cfg(feature = "dma")]
fn release_dma_frames(ppns : &[mm_api::addr::PhysPageNum]) {
    for &ppn in ppns {
        let _ = frame_alloctor::frame_dealloc_result(ppn);
    }
}

#[cfg(feature = "dma")]
struct WaterOsAhciHal;

#[cfg(feature = "dma")]
static AHCI_FALLBACK_CLOCK_MS : AtomicU64 = AtomicU64::new(0);

#[cfg(any(feature = "dma", test))]
const fn simple_ahci_virt_to_phys(va : usize) -> usize {
    let window = va & LOONGARCH64_WINDOW_BASE_MASK;
    if window == LOONGARCH64_UNCACHED_WINDOW_BASE || window == LOONGARCH64_CACHED_WINDOW_BASE {
        va & LOONGARCH64_PHYS_ADDR_MASK
    } else {
        va
    }
}

#[cfg(feature = "dma")]
impl simple_ahci::Hal for WaterOsAhciHal {
    fn virt_to_phys(va : usize) -> usize { simple_ahci_virt_to_phys(va) }

    fn current_ms() -> u64 {
        let Ok(tick) = platform::timer::now_tick() else {
            return AHCI_FALLBACK_CLOCK_MS.fetch_add(1, Ordering::Relaxed);
        };
        let Ok(frequency) = platform::timer::tick_hz() else {
            return AHCI_FALLBACK_CLOCK_MS.fetch_add(1, Ordering::Relaxed);
        };
        if frequency.0 == 0 {
            return AHCI_FALLBACK_CLOCK_MS.fetch_add(1, Ordering::Relaxed);
        }
        let milliseconds = (tick.0 as u128).saturating_mul(1_000) / frequency.0 as u128;
        milliseconds.min(u64::MAX as u128) as u64
    }

    fn flush_dcache() {
        #[cfg(target_arch = "loongarch64")]
        unsafe {
            core::arch::asm!("dbar 0", options(nostack, preserves_flags));
        }
        compiler_fence(Ordering::SeqCst);
    }
}

/// AHCI/SATA 整盘块设备视图（512B 扇区直通）。
#[cfg(feature = "dma")]
pub struct SataBlock(spin::Mutex<simple_ahci::AhciDriver<WaterOsAhciHal>>);

#[cfg(feature = "dma")]
impl SataBlock {
    /// 在给定 AHCI HBA MMIO 窗口上探测并初始化；失败映射为 [`DriverError::IoError`]。
    pub fn new(base : usize, _size : usize) -> DriverResult<Self> {
        log_hba_state(base, "before-init");
        let mmio_base = dma_phys_to_kernel_va(base).ok_or(DriverError::InvalidParam)?;
        log::info!("[ahci] probing simple-ahci mmio_pa={:#x} mmio_va={:#x}", base, mmio_base);
        let Some(ahci) = (unsafe { simple_ahci::AhciDriver::<WaterOsAhciHal>::try_new(mmio_base) })
        else {
            log_hba_state(base, "after-init-failed");
            return Err(DriverError::IoError);
        };
        if ahci.capacity() == 0 {
            log::warn!("[ahci] simple-ahci returned zero capacity after IDENTIFY");
            log_hba_state(base, "after-init-zero-capacity");
            return Err(DriverError::IoError);
        }
        log::info!("[ahci] simple-ahci ready blocks={} block_size={}",
                   ahci.capacity(),
                   ahci.block_size());
        log_hba_state(base, "after-init-success");
        Ok(Self(spin::Mutex::new(ahci)))
    }
}

#[cfg(feature = "dma")]
impl BlockDevice for SataBlock {
    fn total_blocks(&self) -> Option<u64> {
        Some(self.0.lock().capacity())
    }

    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        let Some(_) = sector_count_for(buf.len()) else {
            return Err(DriverError::InvalidParam);
        };
        if !self.0.lock().read(start_block.0, buf) {
            return Err(DriverError::IoError);
        }
        Ok(())
    }

    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let Some(_) = sector_count_for(buf.len()) else {
            return Err(DriverError::InvalidParam);
        };
        if !self.0.lock().write(start_block.0, buf) {
            return Err(DriverError::IoError);
        }
        Ok(())
    }

    fn flush(&mut self) -> DriverResult<()> {
        // 轮询 DMA 写在返回前已完成，无需额外 flush。
        Ok(())
    }
}

/// PCI 端口 I/O 占位：2K1000 只用 MemoryMapped 访问，端口方法为 no-op。
struct UnusedPort;

impl pci::PortOps for UnusedPort {
    unsafe fn read8(&self, _port : u16) -> u8 { 0 }
    unsafe fn read16(&self, _port : u16) -> u16 { 0 }
    unsafe fn read32(&self, _port : u16) -> u32 { 0 }
    unsafe fn write8(&self, _port : u16, _val : u8) {}
    unsafe fn write16(&self, _port : u16, _val : u16) {}
    unsafe fn write32(&self, _port : u16, _val : u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_sata_mass_storage() {
        assert!(is_sata_mass_storage_class(0x01, 0x06));
        assert!(!is_sata_mass_storage_class(0x01, 0x01));
        assert!(!is_sata_mass_storage_class(0x02, 0x06));
        assert!(!is_sata_mass_storage_class(0, 0));
    }

    #[test]
    fn sector_count_rejects_partial_blocks() {
        assert_eq!(sector_count_for(0), Some(0));
        assert_eq!(sector_count_for(BLOCK_SIZE), Some(1));
        assert_eq!(sector_count_for(BLOCK_SIZE * 4),
                   Some(4));
        assert_eq!(sector_count_for(BLOCK_SIZE - 1), None);
        assert_eq!(sector_count_for(BLOCK_SIZE + 1), None);
    }

    #[test]
    fn dma_pages_round_up_and_reject_zero() {
        assert_eq!(dma_pages_for(0), None);
        assert_eq!(dma_pages_for(1), Some(1));
        assert_eq!(dma_pages_for(4096), Some(1));
        assert_eq!(dma_pages_for(4097), Some(2));
        assert_eq!(dma_pages_for(usize::MAX - 1), None);
    }

    #[test]
    fn soc_sata_preparation_enables_phy_and_preserves_coherent_dma() {
        let tuning = 0x1234_5678_9ABC_DEF0 | SATA_CONFIG_PHY_POWER_DOWN | SATA_CONFIG_DMA_COHERENT;
        let prepared = prepared_soc_sata_config(tuning);
        assert_eq!(prepared & SATA_CONFIG_PHY_POWER_DOWN, 0);
        assert_eq!(prepared & SATA_CONFIG_LANE0_RESET_N,
                   SATA_CONFIG_LANE0_RESET_N);
        assert_eq!(prepared & SATA_CONFIG_PHY_RESET_N,
                   SATA_CONFIG_PHY_RESET_N);
        assert_eq!(prepared & SATA_CONFIG_REF_CLOCK_ENABLE,
                   SATA_CONFIG_REF_CLOCK_ENABLE);
        assert_eq!(prepared & SATA_CONFIG_DMA_COHERENT,
                   tuning & SATA_CONFIG_DMA_COHERENT);
        let changed = SATA_CONFIG_PHY_POWER_DOWN |
                      SATA_CONFIG_LANE0_RESET_N |
                      SATA_CONFIG_PHY_RESET_N |
                      SATA_CONFIG_REF_CLOCK_ENABLE;
        assert_eq!(prepared & !changed, tuning & !changed);
    }

    #[test]
    fn dma_address_conversion_preserves_physical_address() {
        let pa = 0x9000_3000;
        let va = dma_phys_to_kernel_va(pa).expect("physical address should fit");
        assert_eq!(va, 0x8000_0000_9000_3000);
        assert_eq!(dma_kernel_va_to_phys(va), Some(pa));
        assert_eq!(dma_kernel_va_to_phys(pa), None);
    }

    #[test]
    fn simple_ahci_hal_translates_direct_windows_only() {
        let pa = 0x0B00_3000;
        assert_eq!(simple_ahci_virt_to_phys(LOONGARCH64_UNCACHED_WINDOW_BASE | pa), pa);
        assert_eq!(simple_ahci_virt_to_phys(LOONGARCH64_CACHED_WINDOW_BASE | pa), pa);
        assert_eq!(simple_ahci_virt_to_phys(pa), pa);
    }
}
