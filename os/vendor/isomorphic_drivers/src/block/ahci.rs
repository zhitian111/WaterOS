//! Driver for AHCI
//!
//! Spec: https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/serial-ata-ahci-spec-rev1-3-1.pdf

use alloc::string::String;
use alloc::vec::Vec;
use core::hint::spin_loop;
use core::marker::PhantomData;
use core::mem::size_of;
use core::slice;
use core::sync::atomic::{fence, Ordering};

use bit_field::*;
use bitflags::*;
use volatile::Volatile;

use crate::provider::Provider;

const AHCI_POLL_SPINS : usize = 10_000_000;
const AHCI_LINK_WAIT_MS : usize = 1_000;
const AHCI_HOST_RESET_TIMEOUT_MS : usize = 1_000;
/// AHCI requires DET=1 to remain asserted for at least 1 ms. Use a wider
/// hardware-timed margin during board bring-up.
const AHCI_COMRESET_ASSERT_MS : usize = 10;
const AHCI_SOFTRESET_TIMEOUT_MS : usize = 500;
const AHCI_MAX_COMMAND_SLOTS : usize = 32;
const AHCI_COMMAND_LIST_OFFSET : usize = 0x000;
const AHCI_RECEIVED_FIS_OFFSET : usize = 0x400;
const AHCI_COMMAND_TABLE_OFFSET : usize = 0x500;
const AHCI_RFIS_SENTINEL : u8 = 0xA5;
const GHC_HR : u32 = 1 << 0;
const GHC_AE : u32 = 1 << 31;
#[inline(always)]
fn dma_io_completion_barrier() {
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        // Rust's SeqCst fence lowers to `dbar 0x10`, an ordering barrier.
        // AHCI needs a completion barrier before a doorbell can trigger DMA;
        // use the strongest form while bringing up the 2K1000 integration.
        core::arch::asm!("dbar 0",
                         options(nostack, preserves_flags));
    }

    #[cfg(not(target_arch = "loongarch64"))]
    fence(Ordering::SeqCst);
}

fn spin_until<F : FnMut() -> bool>(mut ready : F) -> bool {
    for _ in 0..AHCI_POLL_SPINS {
        if ready() {
            return true;
        }
        spin_loop();
    }
    false
}

///
pub struct AHCI<P : Provider> {
    header : usize,
    size : usize,
    provider : PhantomData<P>,
    ghc : &'static mut AHCIGenericHostControl,
    received_fis : &'static mut AHCIReceivedFIS,
    cmd_list : &'static mut [AHCICommandHeader],
    cmd_table : &'static mut AHCICommandTable,
    data : &'static mut [u8],
    port : &'static mut AHCIPort,
}

/// AHCI Generic Host Control (3.1)
#[repr(C)]
struct AHCIGenericHostControl {
    /// Host capability
    capability : Volatile<AHCICap>,
    /// Global host control
    global_host_control : Volatile<u32>,
    /// Interrupt status
    interrupt_status : Volatile<u32>,
    /// Port implemented
    port_implemented : Volatile<u32>,
    /// Version
    version : Volatile<u32>,
    /// Command completion coalescing control
    ccc_control : Volatile<u32>,
    /// Command completion coalescing ports
    ccc_ports : Volatile<u32>,
    /// Enclosure management location
    em_location : Volatile<u32>,
    /// Enclosure management control
    em_control : Volatile<u32>,
    /// Host capabilities extended
    capabilities2 : Volatile<u32>,
    /// BIOS/OS handoff control and status
    bios_os_handoff_control : Volatile<u32>,
}

bitflags! {
    struct AHCICap : u32 {
        const S64A = 1 << 31;
        const SNCQ = 1 << 30;
        const SSNTF = 1 << 29;
        const SMPS = 1 << 28;
        const SSS = 1 << 27;
        const SALP = 1 << 26;
        const SAL = 1 << 25;
        const SCLO = 1 << 24;
        const ISS_GEN_1 = 1 << 20;
        const ISS_GEN_2 = 2 << 20;
        const ISS_GEN_3 = 3 << 20;
        const SAM = 1 << 18;
        const SPM = 1 << 17;
        const FBSS = 1 << 16;
        const PMD = 1 << 15;
        const SSC = 1 << 14;
        const PSC = 1 << 13;
        const CCCS = 1 << 7;
        const EMS = 1 << 6;
        const SXS = 1 << 5;
        // number of ports - 1
        const NUM_MASK = 0b11111;
    }
}

impl AHCIGenericHostControl {
    fn write_global_host_control(&mut self, value : u32) -> u32 {
        dma_io_completion_barrier();
        self.global_host_control
            .write(value);
        dma_io_completion_barrier();
        self.global_host_control
            .read()
    }

    fn log_reset_state(&self, phase : &str) {
        let port = unsafe { &*self.port_ptr(0) };
        info!("AHCI {} HBA cap={:#010x} cap2={:#010x} ghc={:#010x} pi={:#010x}",
              phase,
              self.capability
                  .read()
                  .bits(),
              self.capabilities2
                  .read(),
              self.global_host_control
                  .read(),
              self.port_implemented
                  .read());
        info!("AHCI {} port0 cmd={:#010x} tfd={:#010x} ci={:#010x} is={:#010x}",
              phase,
              port.command.read(),
              port.task_file_data
                  .read(),
              port.command_issue
                  .read(),
              port.interrupt_status
                  .read());
    }

    fn enable_ahci(&mut self) -> bool {
        // ref: Linux ahci_enable_ahci
        for _ in 0..1000 {
            let ctl = self.global_host_control
                          .read();
            if ctl & GHC_AE != 0 {
                return true;
            }
            if self.write_global_host_control(ctl | GHC_AE) & GHC_AE != 0 {
                return true;
            }
            spin_loop();
        }
        false
    }

    fn enable<P : Provider>(&mut self) -> bool {
        // ref: Linux ahci_reset_controller
        self.log_reset_state("before-HR");
        if !self.enable_ahci() {
            warn!("AHCI could not set GHC.AE before host reset");
            return false;
        }

        let ctl = self.global_host_control
                      .read();
        if ctl & GHC_HR == 0 {
            let requested = ctl | GHC_HR;
            let readback = self.write_global_host_control(requested);
            info!("AHCI HR-write requested={:#010x} readback={:#010x}",
                  requested, readback);
        } else {
            info!("AHCI HR already active ghc={:#010x}",
                  ctl);
        }

        let mut reset_complete = false;
        for _ in 0..AHCI_HOST_RESET_TIMEOUT_MS {
            if self.global_host_control
                   .read() &
               GHC_HR ==
               0
            {
                reset_complete = true;
                break;
            }
            P::delay_ms(1);
        }
        if !reset_complete &&
           self.global_host_control
               .read() &
           GHC_HR !=
           0
        {
            warn!("AHCI HBA reset timed out ghc={:#010x}",
                  self.global_host_control
                      .read());
            return false;
        }

        dma_io_completion_barrier();
        info!("AHCI HR-complete ghc={:#010x}",
              self.global_host_control
                  .read());
        if !self.enable_ahci() {
            warn!("AHCI could not restore GHC.AE after host reset");
            return false;
        }
        self.log_reset_state("after-HR");
        true
    }
    fn num_ports(&self) -> usize {
        self.capability
            .read()
            .bits()
            .get_bits(0..5) as usize +
        1
    }
    fn port_ptr(&self, port_num : usize) -> *mut AHCIPort {
        (self as *const _ as usize + 0x100 + 0x80 * port_num) as *mut AHCIPort
    }
}

/// AHCI Port Registers (3.3) (one set per port)
#[repr(C)]
struct AHCIPort {
    // The 2K1000 manual defines CLB/CLBU and FB/FBU as separate 32-bit
    // registers.  Keep them split so MMIO accesses match Linux readl/writel
    // sequences; a 64-bit store is not a portable AHCI register access.
    command_list_base_address_low : Volatile<u32>,
    command_list_base_address_high : Volatile<u32>,
    fis_base_address_low : Volatile<u32>,
    fis_base_address_high : Volatile<u32>,
    interrupt_status : Volatile<u32>,
    interrupt_enable : Volatile<u32>,
    command : Volatile<u32>,
    reserved : Volatile<u32>,
    task_file_data : Volatile<u32>,
    signature : Volatile<u32>,
    sata_status : Volatile<u32>,
    sata_control : Volatile<u32>,
    sata_error : Volatile<u32>,
    sata_active : Volatile<u32>,
    command_issue : Volatile<u32>,
    sata_notification : Volatile<u32>,
    fis_based_switch_control : Volatile<u32>,
}

impl AHCIPort {
    fn preserve_dmacr(&self, port_num : usize) {
        // PxDMACR is at port offset 0x70 on the 2K1000/DWC implementation,
        // beyond the standard AHCI register struct represented above.  DWC
        // platforms may describe transfer-size overrides, but without such
        // platform data Linux preserves the firmware value.  Keep that value
        // during command-fetch debugging instead of replacing 0x66 with the
        // U-Boot driver's i.MX-specific 0x4444 default.
        let dmacr = (self as *const Self as usize + 0x70) as *const u32;
        let value = unsafe { core::ptr::read_volatile(dmacr) };
        info!("AHCI port{} DMACR preserved={:#010x}", port_num, value);
    }

    fn set_dma_bases(&mut self, command_list : u64, fis : u64) {
        // Linux writes the high halves before the low halves when the HBA
        // advertises 64-bit DMA, then flushes through a command read.
        self.command_list_base_address_high
            .write((command_list >> 32) as u32);
        self.command_list_base_address_low
            .write(command_list as u32);
        self.fis_base_address_high
            .write((fis >> 32) as u32);
        self.fis_base_address_low
            .write(fis as u32);
        self.command.read();
    }

    fn command_list_base_address(&self) -> u64 {
        (self.command_list_base_address_low
             .read() as u64) |
        ((self.command_list_base_address_high
              .read() as u64) <<
         32)
    }

    fn fis_base_address(&self) -> u64 {
        (self.fis_base_address_low
             .read() as u64) |
        ((self.fis_base_address_high
              .read() as u64) <<
         32)
    }

    fn log_command_dma_state(&self,
                             phase : &str,
                             header : &AHCICommandHeader,
                             table : &AHCICommandTable,
                             received_fis : &AHCIReceivedFIS) {
        let header_words : [u32; 8] = core::array::from_fn(|index| unsafe {
            core::ptr::read_volatile((header as *const _ as *const u32).add(index))
        });
        let cfis_words : [u32; 5] = core::array::from_fn(|index| unsafe {
            core::ptr::read_volatile((&table.cfis as *const _ as *const u32).add(index))
        });
        let prdt_words : [u32; 4] = core::array::from_fn(|index| unsafe {
            core::ptr::read_volatile((&table.prdt[0] as *const _ as *const u32).add(index))
        });
        let (dma_changed, pio_changed, d2h_changed, sdbfis_changed) = received_fis.changed_counts();

        info!("AHCI {} DMA registers clb={:#018x} fb={:#018x} ci={:#010x} sact={:#010x}",
              phase,
              self.command_list_base_address(),
              self.fis_base_address(),
              self.command_issue
                  .read(),
              self.sata_active
                  .read());
        info!("AHCI {} command header={:08x?}",
              phase, header_words);
        info!("AHCI {} command cfis={:08x?} prdt={:08x?}",
              phase, cfis_words, prdt_words);
        info!("AHCI {} RFIS changed dma={}/{} pio={}/{} d2h={}/{} sdbfis={}/{} d2h={:02x?}",
              phase,
              dma_changed,
              received_fis.dma.len(),
              pio_changed,
              received_fis.pio.len(),
              d2h_changed,
              received_fis.d2h.len(),
              sdbfis_changed,
              received_fis.sdbfis.len(),
              received_fis.d2h);
    }

    fn log_link_state(&self, phase : &str, port_num : usize) {
        let base = self as *const _ as usize;
        let read =
            |offset : usize| unsafe { core::ptr::read_volatile((base + offset) as *const u32) };
        let ssts = read(0x28);
        let tfd = read(0x20);
        info!("AHCI {} port{} cmd={:#010x} tfd={:#010x}(sts={:#04x} err={:#04x}) sig={:#010x} \
               ssts={:#010x}(det={} spd={} ipm={}) sctl={:#010x} serr={:#010x} is={:#010x} \
               ie={:#010x} sact={:#010x} ci={:#010x} sntf={:#010x} fbs={:#010x} dmacr={:#010x} \
               phycr={:#010x} physr={:#010x}",
              phase,
              port_num,
              read(0x18),
              tfd,
              tfd & 0xFF,
              (tfd >> 8) & 0xFF,
              read(0x24),
              ssts,
              ssts & 0xF,
              (ssts >> 4) & 0xF,
              (ssts >> 8) & 0xF,
              read(0x2C),
              read(0x30),
              read(0x10),
              read(0x14),
              read(0x34),
              read(0x38),
              read(0x3C),
              read(0x40),
              read(0x70),
              read(0x78),
              read(0x7C));
    }

    fn link_active(&self) -> bool {
        let sata_status = self.sata_status
                              .read();
        sata_status.get_bits(8..12) == 1 && sata_status.get_bits(0..4) == 3
    }

    fn set_linux_link_power_policy(&mut self, port_num : usize) {
        // libata's default max-performance policy preserves DET/SPD and sets
        // IPM=3.  The working Linux driver reports SControl=0x300 on this
        // board; matching it avoids leaving the DWC link in its reset-time
        // power-management policy while commands are first submitted.
        let before = self.sata_control
                         .read();
        let requested = (before & 0x0F0) | 0x300;
        self.sata_control
            .write(requested);
        let readback = self.sata_control
                           .read();
        info!("AHCI port{} Linux link policy SCTL before={:#010x} requested={:#010x} \
               readback={:#010x}",
              port_num, before, requested, readback);
        self.sata_error
            .write(u32::MAX);
    }

    fn power_up(&mut self, port_num : usize, staggered_spin_up : bool) {
        if staggered_spin_up {
            self.command
                .update(|command| {
                    // PxCMD.SUD: spin up the device.
                    *command |= 1 << 1;
                });
            self.command.read();
        }
        self.command
            .update(|command| {
                // Match libahci's separate ICC_ACTIVE wake-up operation.
                *command &= !(0xF << 28);
                *command |= 1 << 28;
            });
        self.command.read();
        self.log_link_state("post-power", port_num);
    }

    fn comreset<P : Provider>(&mut self, port_num : usize, speed_limit : u32) {
        debug_assert!(speed_limit <= 3);
        // PxSCTL.DET=1 initiates COMRESET. Hold it asserted before returning
        // DET to zero so the PHY can resume normal link negotiation. SPD=0
        // allows the highest common speed; SPD=1 restricts training to Gen1.
        // Use raw volatile accesses here so the immediate readback also proves
        // that the SoC accepted the MMIO write independently of the wrapper.
        let sata_control = (self as *mut Self as usize + 0x2C) as *mut u32;
        let before = unsafe { core::ptr::read_volatile(sata_control) };
        let asserted = (before & !0xFF) | (speed_limit << 4) | 1;
        unsafe {
            core::ptr::write_volatile(sata_control, asserted);
        }
        let asserted_readback = unsafe { core::ptr::read_volatile(sata_control) };
        info!("AHCI port{} PxSCTL assert raw speed_limit={} before={:#010x} requested={:#010x} \
               readback={:#010x}",
              port_num, speed_limit, before, asserted, asserted_readback);
        self.log_link_state("comreset-asserted", port_num);
        P::delay_ms(AHCI_COMRESET_ASSERT_MS);
        // Build the release value from what was requested. Some DWC revisions
        // expose DET as self-clearing, so its immediate readback is not a safe
        // source for the persistent SPD/IPM fields.
        let released = asserted & !0xF;
        unsafe {
            core::ptr::write_volatile(sata_control, released);
        }
        let released_readback = unsafe { core::ptr::read_volatile(sata_control) };
        info!("AHCI port{} PxSCTL release requested={:#010x} readback={:#010x}",
              port_num, released, released_readback);
        self.sata_error
            .write(u32::MAX);
        self.log_link_state("comreset-released", port_num);
    }

    fn spin_on_slot(&mut self, slot : usize) -> bool {
        let completed = spin_until(|| {
            !self.command_issue
                 .read()
                 .get_bit(slot)
        });
        if completed {
            // Order device DMA writes before consuming received FIS or data.
            fence(Ordering::SeqCst);
        }
        completed
    }

    fn wait_slot_ms<P : Provider>(&mut self, slot : usize, timeout_ms : usize) -> bool {
        for _ in 0..timeout_ms {
            if !self.command_issue
                    .read()
                    .get_bit(slot)
            {
                fence(Ordering::SeqCst);
                return true;
            }
            P::delay_ms(1);
        }
        false
    }

    fn kick_engine<P : Provider>(&mut self, port_num : usize, supports_clo : bool) -> bool {
        // Linux samples TFD before stopping the engine.  Keep that sample so
        // CLO decisions reflect the state that caused the reset recovery,
        // rather than a possibly changed value after CR drops.
        let task_file_before = self.task_file_data
                                   .read();
        let command_before = self.command.read();
        info!("AHCI port{} pre-kick cmd={:#010x} tfd={:#010x}(sts={:#04x} err={:#04x}) clo={} \
               busy={}",
              port_num,
              command_before,
              task_file_before,
              task_file_before & 0xFF,
              (task_file_before >> 8) & 0xFF,
              supports_clo,
              task_file_before & (0x80 | 0x08) != 0);
        self.command
            .update(|command| *command &= !(1 << 0));
        let command_stopped_writeback = self.command.read();
        if !spin_until(|| self.command.read() & (1 << 15) == 0) {
            warn!("AHCI port{} cannot stop command engine before software reset",
                  port_num);
            return false;
        }
        let command_stopped = self.command.read();
        info!("AHCI port{} engine-stopped cmd-writeback={:#010x} cmd={:#010x} tfd={:#010x}",
              port_num,
              command_stopped_writeback,
              command_stopped,
              self.task_file_data
                  .read());

        if task_file_before & (0x80 | 0x08) != 0 {
            if !supports_clo {
                warn!("AHCI port{} is busy before software reset and CAP.CLO is absent",
                      port_num);
                return false;
            }
            self.command
                .update(|command| *command |= 1 << 3);
            let clo_writeback = self.command.read();
            info!("AHCI port{} CLO requested cmd={:#010x} tfd={:#010x}",
                  port_num,
                  clo_writeback,
                  self.task_file_data
                      .read());
            if !spin_until(|| self.command.read() & (1 << 3) == 0) {
                warn!("AHCI port{} command-list override timed out",
                      port_num);
                return false;
            }
            info!("AHCI port{} CLO complete cmd={:#010x} tfd={:#010x}",
                  port_num,
                  self.command.read(),
                  self.task_file_data
                      .read());
        }

        self.command
            .update(|command| *command |= 1 << 0);
        let command_restarted = self.command.read();
        info!("AHCI port{} engine-restarted cmd={:#010x} tfd={:#010x}",
              port_num,
              command_restarted,
              self.task_file_data
                  .read());
        true
    }

    fn software_reset<P : Provider>(&mut self,
                                    port_num : usize,
                                    supports_clo : bool,
                                    header : &mut AHCICommandHeader,
                                    table : &mut AHCICommandTable,
                                    received_fis : &mut AHCIReceivedFIS)
                                    -> bool {
        if !self.kick_engine::<P>(port_num, supports_clo) {
            return false;
        }

        table.cfis
             .prepare_softreset(true);
        header.flags = REGISTER_H2D_FIS_DWORDS |
                       CommandHeaderFlags::RESET.bits() |
                       CommandHeaderFlags::CLEAR.bits();
        header.prdt_length = 0;
        header.prd_byte_count = 0;
        self.interrupt_status
            .write(u32::MAX);
        self.sata_error
            .write(u32::MAX);
        received_fis.fill_sentinel();
        info!("AHCI port{} software-reset assert flags={:#06x} cfis0={:#010x} control={:#04x}",
              port_num,
              header.flags,
              unsafe { core::ptr::read_unaligned(&table.cfis as *const _ as *const u32) },
              table.cfis.control);
        self.log_command_dma_state("softreset-assert-before-ci",
                                   header,
                                   table,
                                   received_fis);
        self.issue_command(0);
        if !self.wait_slot_ms::<P>(0, AHCI_SOFTRESET_TIMEOUT_MS) {
            warn!("AHCI port{} software-reset assert FIS timed out",
                  port_num);
            self.log_link_state("softreset-assert-timeout", port_num);
            self.log_command_dma_state("softreset-assert-timeout",
                                       header,
                                       table,
                                       received_fis);
            return false;
        }
        self.log_command_dma_state("softreset-assert-complete",
                                   header,
                                   table,
                                   received_fis);

        P::delay_ms(1);
        table.cfis
             .prepare_softreset(false);
        header.flags = REGISTER_H2D_FIS_DWORDS;
        header.prd_byte_count = 0;
        received_fis.fill_sentinel();
        info!("AHCI port{} software-reset release flags={:#06x} cfis0={:#010x} control={:#04x}",
              port_num,
              header.flags,
              unsafe { core::ptr::read_unaligned(&table.cfis as *const _ as *const u32) },
              table.cfis.control);
        self.log_command_dma_state("softreset-release-before-ci",
                                   header,
                                   table,
                                   received_fis);
        self.issue_command(0);
        // Linux submits the deassert FIS without waiting on PxCI, then waits
        // for the reset device to become ready. During reset the slot may stay
        // active longer than the task-file busy state, so PxCI is not the
        // readiness condition for this second FIS.
        self.log_command_dma_state("softreset-release-submitted",
                                   header,
                                   table,
                                   received_fis);

        for _ in 0..AHCI_SOFTRESET_TIMEOUT_MS {
            if self.task_file_data
                   .read() &
               0x80 ==
               0
            {
                self.log_link_state("softreset-complete", port_num);
                return true;
            }
            P::delay_ms(1);
        }
        warn!("AHCI port{} device stayed busy after software reset",
              port_num);
        self.log_link_state("softreset-busy-timeout", port_num);
        false
    }

    fn issue_command(&mut self, slot : usize) {
        assert!(slot < 32);
        // Publish command-table writes before ringing the MMIO doorbell. On
        // LoongArch this must be a completion barrier, not only the ordering
        // barrier emitted by core::sync::atomic::fence.
        dma_io_completion_barrier();
        self.command_issue
            .write(1 << (slot as u32));
        dma_io_completion_barrier();
        // Flush the posted MMIO write on integrations that require it.
        self.command_issue
            .read();
    }
}

fn wait_for_active_port<P : Provider>(ghc : &AHCIGenericHostControl,
                                      num_ports : usize,
                                      port_map : u32)
                                      -> Option<usize> {
    for _ in 0..AHCI_LINK_WAIT_MS {
        if let Some(port_num) = (0..num_ports).find(|&i| {
                                                  port_map & (1 << i) != 0 &&
                                                  unsafe { &*ghc.port_ptr(i) }.link_active()
                                              })
        {
            return Some(port_num);
        }
        P::delay_ms(1);
    }
    None
}

/// AHCI Received FIS Structure (4.2.1)
#[repr(C)]
struct AHCIReceivedFIS {
    dma : [u8; 0x20],
    pio : [u8; 0x20],
    d2h : [u8; 0x18],
    sdbfis : [u8; 0x8],
    ufis : [u8; 0x40],
    reserved : [u8; 0x60],
}

impl AHCIReceivedFIS {
    /// Poison every HBA-owned receive area before publishing a command.
    ///
    /// Allocator zeroing is not evidence that the controller wrote a FIS: a
    /// recycled frame may still contain an older command's data.
    fn fill_sentinel(&mut self) {
        self.dma.fill(AHCI_RFIS_SENTINEL);
        self.pio.fill(AHCI_RFIS_SENTINEL);
        self.d2h.fill(AHCI_RFIS_SENTINEL);
        self.sdbfis.fill(AHCI_RFIS_SENTINEL);
        self.ufis.fill(AHCI_RFIS_SENTINEL);
        self.reserved.fill(AHCI_RFIS_SENTINEL);
    }

    fn changed_counts(&self) -> (usize, usize, usize, usize) {
        let count = |bytes : &[u8]| bytes.iter().filter(|byte| **byte != AHCI_RFIS_SENTINEL).count();
        (count(&self.dma), count(&self.pio), count(&self.d2h), count(&self.sdbfis))
    }
}

/// # AHCI Command List Structure (4.2.2)
///
/// Host sends commands to the device through Command List.
///
/// Command List consists of 1 to 32 command headers, each one is called a slot.
///
/// Each command header describes an ATA or ATAPI command, including a
/// Command FIS, an ATAPI command buffer and a bunch of Physical Region
/// Descriptor Tables specifying the data payload address and size.
///
/// https://wiki.osdev.org/images/e/e8/Command_list.jpg
#[repr(C)]
struct AHCICommandHeader {
    /// PMP R C B R P W A CFL
    flags : u16,
    /// Physical region descriptor table length in entries
    prdt_length : u16,
    /// Physical region descriptor byte count transferred
    prd_byte_count : u32,
    /// Command table descriptor base address
    command_table_base_address : u64,
    /// Reserved
    reserved : [u32; 4],
}

bitflags! {
    struct CommandHeaderFlags: u16 {
        /// Command FIS length in DWORDS, 2 ~ 16
        const CFL_MASK = 0b11111;
        /// ATAPI
        const ATAPI = 1 << 5;
        /// Write, 1: H2D, 0: D2H
        const WRITE = 1 << 6;
        /// Prefetchable
        const PREFETCHABLE = 1 << 7;
        /// Reset
        const RESET = 1 << 8;
        /// BIST
        const BIST = 1 << 9;
        /// Clear busy upon R_OK
        const CLEAR = 1 << 10;
        /// Port multiplier port
        const PORT_MULTIPLIER_PORT_MASK = 0b1111 << 12;
    }
}

/// AHCI Command Table (4.2.3)
#[repr(C)]
struct AHCICommandTable {
    /// Command FIS
    cfis : SATAFISRegH2D,
    /// ATAPI command, 12 or 16 bytes
    acmd : [u8; 16],
    /// Reserved
    reserved : [u8; 48],
    /// Physical region descriptor table entries, 0 ~ 65535
    prdt : [AHCIPrdtEntry; 1],
}

/// Physical region descriptor table entry
#[repr(C)]
struct AHCIPrdtEntry {
    /// Data base address
    data_base_address : u64,
    /// Reserved
    reserved : u32,
    /// Bit 21-0: Byte count, 4M max
    /// Bit 31:   Interrupt on completion
    byte_count_i : u32,
}

const FIS_REG_H2D : u8 = 0x27;

/// Obsolete ATA task-file bits which software must preserve as one.
/// `ata_tf_init()` supplies the same defaults before Linux builds an AHCI FIS.
const ATA_DEVICE_OBS : u8 = (1 << 7) | (1 << 5);
const ATA_DEVCTL_OBS : u8 = 1 << 3;
const ATA_SRST : u8 = 1 << 2;

const CMD_READ_DMA_EXT : u8 = 0x25;
const CMD_WRITE_DMA_EXT : u8 = 0x35;
/// ATA CHECK POWER MODE is a harmless register-only command.  It deliberately
/// carries no PRDT so it can separate command-list/FIS DMA from data DMA.
const CMD_CHECK_POWER_MODE : u8 = 0xE5;
const CMD_IDENTIFY_DEVICE : u8 = 0xEC;
/// A Register Host-to-Device FIS is 20 bytes, or five DWORDs.
const REGISTER_H2D_FIS_DWORDS : u16 = 5;

/// SATA Register FIS - Host to Device
///
/// https://wiki.osdev.org/AHCI Figure 5-2
#[repr(C)]
struct SATAFISRegH2D {
    fis_type : u8,
    cflags : u8,
    command : u8,
    feature_lo : u8,

    lba_0 : u8, // LBA 7:0
    lba_1 : u8, // LBA 15:8
    lba_2 : u8, // LBA 23:16
    dev_head : u8,

    lba_3 : u8, // LBA 31:24
    lba_4 : u8, // LBA 39:32
    lba_5 : u8, // LBA 47:40
    feature_hi : u8,

    sector_count : u16,
    reserved : u8,
    control : u8,

    _padding : [u8; 48],
}

impl SATAFISRegH2D {
    fn prepare_register_h2d(&mut self, is_command : bool) {
        unsafe {
            core::ptr::write_bytes(self as *mut Self, 0, 1);
        }
        self.fis_type = FIS_REG_H2D;
        self.cflags = if is_command { 1 << 7 } else { 0 };
        self.dev_head = ATA_DEVICE_OBS;
        self.control = ATA_DEVCTL_OBS;
    }

    fn prepare_softreset(&mut self, asserted : bool) {
        self.prepare_register_h2d(false);
        if asserted {
            self.control |= ATA_SRST;
        }
    }

    fn set_lba(&mut self, lba : u64) {
        self.lba_0 = (lba >> 0) as u8;
        self.lba_1 = (lba >> 8) as u8;
        self.lba_2 = (lba >> 16) as u8;
        self.lba_3 = (lba >> 24) as u8;
        self.lba_4 = (lba >> 32) as u8;
        self.lba_5 = (lba >> 40) as u8;
    }
}

/// IDENTIFY DEVICE data
///
/// ATA8-ACS Table 29
#[repr(C)]
struct ATAIdentifyPacket {
    _1 : [u16; 10],
    serial : [u8; 20], // words 10-19
    _2 : [u16; 3],
    firmware : [u8; 8], // words 23-26
    model : [u8; 40],   // words 27-46
    _3 : [u16; 13],
    lba_sectors : u32, // words 60-61
    _4 : [u16; 38],
    lba48_sectors : u64, // words 100-103
}

impl<P : Provider> AHCI<P> {
    /// Initialize an AHCI controller using the hardware-provided PI register.
    pub fn new(header : usize, size : usize) -> Option<Self> {
        Self::new_inner(header, size, None, false, false)
    }

    /// Initialize an AHCI controller using an explicit platform port map.
    ///
    /// Some firmware leaves PI unset, or a host reset clears it. The caller may
    /// provide the board topology here; the value is restored after reset and
    /// also used as the software probe mask if PI is not writable.
    pub fn new_with_port_map(header : usize, size : usize, port_map : u32) -> Option<Self> {
        Self::new_inner(header,
                        size,
                        Some(port_map),
                        false,
                        false)
    }

    /// Initialize a controller whose integration requires software to expose
    /// staggered spin-up in CAP before PxCMD.SUD becomes writable.
    ///
    /// Synopsys DWC integrations may implement CAP as a writable synthesis
    /// shadow register.  On ordinary AHCI controllers CAP is read-only, so
    /// keep this quirk explicit instead of applying the write globally.
    pub fn new_with_port_map_and_staggered_spin_up(header : usize,
                                                   size : usize,
                                                   port_map : u32)
                                                   -> Option<Self> {
        Self::new_inner(header,
                        size,
                        Some(port_map),
                        true,
                        false)
    }

    /// Initialize a staggered-spin-up integration whose firmware handoff is
    /// already task-file ready, without issuing ATA SRST before IDENTIFY.
    ///
    /// This is an explicit platform quirk: ordinary AHCI users retain the
    /// standard software-reset path. It is also useful for separating an
    /// SRST-specific failure from a generic command-DMA failure.
    pub fn new_with_port_map_and_staggered_spin_up_without_soft_reset(header : usize,
                                                                      size : usize,
                                                                      port_map : u32)
                                                                      -> Option<Self> {
        Self::new_inner(header, size, Some(port_map), true, true)
    }

    fn new_inner(header : usize,
                 size : usize,
                 forced_port_map : Option<u32>,
                 force_staggered_spin_up : bool,
                 skip_software_reset : bool)
                 -> Option<Self> {
        let ghc = unsafe { &mut *(header as *mut AHCIGenericHostControl) };
        let initial_port_map = ghc.port_implemented
                                  .read();

        if !ghc.enable::<P>() {
            warn!("AHCI controller reset/enable timed out");
            return None;
        }

        if force_staggered_spin_up {
            let cap_before = ghc.capability
                                .read();
            ghc.capability
               .write(cap_before | AHCICap::SSS);
            let cap_after = ghc.capability
                               .read();
            info!("AHCI CAP.SSS quirk before={:#010x} requested={:#010x} readback={:#010x}",
                  cap_before.bits(),
                  (cap_before | AHCICap::SSS).bits(),
                  cap_after.bits());
            if !cap_after.contains(AHCICap::SSS) {
                warn!("AHCI CAP.SSS quirk was not accepted; PxCMD.SUD may remain read-only");
            }
        }

        let num_ports = ghc.num_ports();
        let valid_port_mask = if num_ports >= 32 {
            u32::MAX
        } else {
            (1u32 << num_ports) - 1
        };
        let port_map = forced_port_map.unwrap_or(initial_port_map) & valid_port_mask;
        if port_map == 0 {
            warn!("AHCI has no implemented ports (CAP.NP={}, PI={:#x})",
                  num_ports - 1,
                  initial_port_map);
            return None;
        }

        // Linux likewise saves PI before reset and restores it afterwards.
        // Keep using the requested software mask if a controller exposes PI as
        // read-only, but report the mismatch for platform diagnosis.
        ghc.port_implemented
           .write(port_map);
        let restored_port_map = ghc.port_implemented
                                   .read();
        if restored_port_map != port_map {
            warn!("AHCI PI restore mismatch requested={:#x} readback={:#x}",
                  port_map, restored_port_map);
        }
        ghc.log_reset_state("after-quirks");

        let staggered_spin_up = ghc.capability
                                   .read()
                                   .contains(AHCICap::SSS);
        for port_num in 0..num_ports {
            if port_map & (1 << port_num) == 0 {
                continue;
            }
            let port = unsafe { &mut *ghc.port_ptr(port_num) };
            port.log_link_state("pre-power", port_num);
            port.power_up(port_num, staggered_spin_up);
        }

        // A SoC-level PHY/lane reset invalidates the device state inherited
        // from firmware even when SSTS has already returned to DET=3.  Always
        // perform the SATA hard-reset handshake before submitting an ATA FIS.
        for port_num in 0..num_ports {
            if port_map & (1 << port_num) != 0 {
                let port = unsafe { &mut *ghc.port_ptr(port_num) };
                info!("AHCI forcing COMRESET on port{}", port_num);
                port.comreset::<P>(port_num, 0);
            }
        }

        let mut active_port = wait_for_active_port::<P>(ghc, num_ports, port_map);
        if active_port.is_none() {
            for port_num in 0..num_ports {
                if port_map & (1 << port_num) != 0 {
                    let port = unsafe { &mut *ghc.port_ptr(port_num) };
                    port.log_link_state("unrestricted-link-timeout", port_num);
                    port.comreset::<P>(port_num, 1);
                }
            }
            warn!("AHCI unrestricted link training failed; retrying port map {:#x} at Gen1",
                  port_map);
            active_port = wait_for_active_port::<P>(ghc, num_ports, port_map);
        }
        if active_port.is_none() {
            for port_num in 0..num_ports {
                if port_map & (1 << port_num) != 0 {
                    unsafe { &mut *ghc.port_ptr(port_num) }.log_link_state("gen1-link-timeout",
                                                                           port_num);
                }
            }
            warn!("AHCI no active SATA link on port map {:#x}, including Gen1 retry",
                  port_map);
            return None;
        }

        if let Some(port_num) = active_port {
            let port = unsafe { &mut *ghc.port_ptr(port_num) };
            port.log_link_state("link-active", port_num);
            let mut task_file_ready = false;
            for _ in 0..AHCI_LINK_WAIT_MS {
                if port.task_file_data.read() & (0x80 | 0x08) == 0 {
                    task_file_ready = true;
                    break;
                }
                P::delay_ms(1);
            }
            if !task_file_ready {
                warn!("AHCI port{} task file stayed busy after forced COMRESET", port_num);
                port.log_link_state("post-comreset-busy-timeout", port_num);
                return None;
            }
            port.log_link_state("post-comreset-ready", port_num);
            port.set_linux_link_power_policy(port_num);

            debug!("AHCI probing port {}", port_num);
            // Disable Port First
            // ref: Linux ahci_stop_engine
            port.command
                .update(|c| {
                    // ST
                    c.set_bit(0, false);
                });
            // LIST_ON
            if !spin_until(|| port.command.read() & (1 << 15) == 0) {
                warn!("AHCI command-list engine stop timed out");
                return None;
            }
            // ref: Linux ahci_stop_fis_rx
            port.command
                .update(|c| {
                    // FRE
                    c.set_bit(4, false);
                });
            // FIS_ON
            if !spin_until(|| port.command.read() & (1 << 14) == 0) {
                warn!("AHCI FIS receive engine stop timed out");
                return None;
            }

            // Preserve the board firmware's DWC DMA transfer-size settings.
            // The command doorbell path supplies its own completion barrier.
            port.preserve_dmacr(port_num);

            // Match Linux's coherent per-port layout. The command list needs
            // 1 KiB alignment, RFIS 256-byte alignment and command tables
            // 128-byte alignment; all three live in one control page.
            let (control_va, control_pa) = P::alloc_dma(P::PAGE_SIZE);
            let (data_va, data_pa) = P::alloc_dma(P::PAGE_SIZE);

            if control_va == 0 ||
               control_pa == 0 ||
               data_va == 0 ||
               data_pa == 0 ||
               P::PAGE_SIZE < AHCI_COMMAND_TABLE_OFFSET + size_of::<AHCICommandTable>() ||
               control_pa & 0x3FF != 0
            {
                warn!("AHCI DMA allocation/layout invalid control={:#x}/{:#x} data={:#x}/{:#x} \
                       page={:#x}",
                      control_va,
                      control_pa,
                      data_va,
                      data_pa,
                      P::PAGE_SIZE);
                if control_va != 0 {
                    P::dealloc_dma(control_va, P::PAGE_SIZE);
                }
                if data_va != 0 {
                    P::dealloc_dma(data_va, P::PAGE_SIZE);
                }
                return None;
            }

            let cmd_list_va = control_va + AHCI_COMMAND_LIST_OFFSET;
            let cmd_list_pa = control_pa + AHCI_COMMAND_LIST_OFFSET;
            let rfis_va = control_va + AHCI_RECEIVED_FIS_OFFSET;
            let rfis_pa = control_pa + AHCI_RECEIVED_FIS_OFFSET;
            let cmd_table_va = control_va + AHCI_COMMAND_TABLE_OFFSET;
            let cmd_table_pa = control_pa + AHCI_COMMAND_TABLE_OFFSET;

            info!("AHCI DMA control={:#x}/{:#x} cmd-list={:#x}/{:#x} rfis={:#x}/{:#x} \
                   cmd-table={:#x}/{:#x} data={:#x}/{:#x}",
                  control_va,
                  control_pa,
                  cmd_list_va,
                  cmd_list_pa,
                  rfis_va,
                  rfis_pa,
                  cmd_table_va,
                  cmd_table_pa,
                  data_va,
                  data_pa);

            // Do not rely on allocator zeroing or a recycled frame's contents
            // while diagnosing DMA visibility.  The HBA owns these pages once
            // PxCI is published.
            unsafe {
                core::ptr::write_bytes(control_va as *mut u8, 0, P::PAGE_SIZE);
                core::ptr::write_bytes(data_va as *mut u8,
                                       AHCI_RFIS_SENTINEL,
                                       P::PAGE_SIZE);
            }

            let received_fis = unsafe { &mut *(rfis_va as *mut AHCIReceivedFIS) };
            let cmd_list = unsafe {
                slice::from_raw_parts_mut(cmd_list_va as *mut AHCICommandHeader,
                                          AHCI_MAX_COMMAND_SLOTS)
            };
            let cmd_table = unsafe { &mut *(cmd_table_va as *mut AHCICommandTable) };

            cmd_table.prdt[0].data_base_address = data_pa as u64;
            cmd_table.prdt[0].byte_count_i = (BLOCK_SIZE - 1) as u32;

            cmd_list[0].command_table_base_address = cmd_table_pa as u64;
            cmd_list[0].prdt_length = 1;
            cmd_list[0].prd_byte_count = 0;
            cmd_list[0].flags = REGISTER_H2D_FIS_DWORDS;

            port.set_dma_bases(cmd_list_pa as u64, rfis_pa as u64);

            // clear errors
            port.sata_error
                .write(0xFFFFFFFF);

            // ref: Linux ahci_start_fis_rx
            // enable fre
            port.command
                .update(|c| {
                    // FRE
                    *c |= 1 << 4;
                });
            // flush
            port.command.read();

            // ref: Linux ahci_start_engine
            // enable port
            port.command
                .update(|c| {
                    // ST
                    *c |= 1 << 0;
                });
            // flush
            port.command.read();

            // wait for ST
            if !spin_until(|| port.command.read() & (1 << 0) != 0) {
                warn!("AHCI command-list engine start timed out");
                return None;
            }

            let stat = port.sata_status
                           .read();
            if stat == 0 {
                warn!("port is not connected to external drive?");
                return None;
            }

            if skip_software_reset {
                let task_file = port.task_file_data
                                    .read();
                info!("AHCI port{} skipping software reset for direct IDENTIFY tfd={:#010x} \
                       (sts={:#04x} err={:#04x})",
                      port_num,
                      task_file,
                      task_file & 0xFF,
                      (task_file >> 8) & 0xFF);
            } else {
                let supports_clo = ghc.capability
                                      .read()
                                      .contains(AHCICap::SCLO);
                if !port.software_reset::<P>(port_num,
                                             supports_clo,
                                             &mut cmd_list[0],
                                             cmd_table,
                                             received_fis)
                {
                    warn!("AHCI port{} device software reset failed",
                          port_num);
                    return None;
                }
            }

            // First exercise the same command-list/FIS fetch path with a
            // register-only ATA command.  If this slot never completes, a
            // PRDT or data-buffer change cannot be the cause of IDENTIFY's
            // failure; retain the full DMA state for that distinction.
            cmd_list[0].flags = REGISTER_H2D_FIS_DWORDS;
            cmd_list[0].prdt_length = 0;
            cmd_list[0].prd_byte_count = 0;
            cmd_table.cfis
                      .prepare_register_h2d(true);
            cmd_table.cfis
                      .command = CMD_CHECK_POWER_MODE;
            port.interrupt_status
                .write(u32::MAX);
            port.sata_error
                .write(u32::MAX);
            received_fis.fill_sentinel();
            info!("AHCI port{} no-data CHECK POWER MODE setup flags={:#06x} cfis0={:#010x}",
                  port_num,
                  cmd_list[0].flags,
                  unsafe {
                      core::ptr::read_unaligned(&cmd_table.cfis as *const _ as *const u32)
                  });
            port.issue_command(0);
            if !port.wait_slot_ms::<P>(0, AHCI_SOFTRESET_TIMEOUT_MS) {
                warn!("AHCI port{} no-data CHECK POWER MODE timed out", port_num);
                port.log_link_state("no-data-timeout", port_num);
                port.log_command_dma_state("no-data-timeout",
                                           &cmd_list[0],
                                           cmd_table,
                                           received_fis);
                return None;
            }
            port.log_command_dma_state("no-data-complete",
                                       &cmd_list[0],
                                       cmd_table,
                                       received_fis);

            cmd_list[0].flags = REGISTER_H2D_FIS_DWORDS;
            cmd_list[0].prdt_length = 1;
            cmd_list[0].prd_byte_count = 0;
            // 7.15 IDENTIFY DEVICE - ECh, PIO Data-In
            let cfis_word = {
                let fis = &mut cmd_table.cfis;
                fis.prepare_register_h2d(true);
                fis.command = CMD_IDENTIFY_DEVICE;
                unsafe { core::ptr::read_unaligned(fis as *const _ as *const u32) }
            };

            if !spin_until(|| {
                port.task_file_data
                    .read() &
                (0x80 | 0x08) ==
                0
            }) {
                warn!("AHCI device stayed busy before IDENTIFY DEVICE");
                return None;
            }
            // Clear stale link-up/FIS status before attributing status to the
            // IDENTIFY command.
            port.interrupt_status
                .write(u32::MAX);
            received_fis.fill_sentinel();
            let data = unsafe { slice::from_raw_parts_mut(data_va as *mut u8, BLOCK_SIZE) };
            data.fill(AHCI_RFIS_SENTINEL);
            info!("AHCI IDENTIFY setup clb={:#x} fb={:#x} ctba={:#x} data={:#x} flags={:#06x} \
                   prdtl={} dbc={:#010x} cfis0={:#010x}",
                  port.command_list_base_address(),
                  port.fis_base_address(),
                  cmd_list[0].command_table_base_address,
                  cmd_table.prdt[0].data_base_address,
                  cmd_list[0].flags,
                  cmd_list[0].prdt_length,
                  cmd_table.prdt[0].byte_count_i,
                  cfis_word);
            port.issue_command(0);
            if !port.spin_on_slot(0) {
                warn!("AHCI IDENTIFY DEVICE timed out");
                port.log_link_state("identify-timeout", port_num);
                port.log_command_dma_state("identify-timeout",
                                           &cmd_list[0],
                                           cmd_table,
                                           received_fis);
                info!("AHCI IDENTIFY timeout header flags={:#06x} prdtl={} prdbc={} ctba={:#x} \
                       cfis0={:#010x} pio={:02x?} d2h={:02x?}",
                      cmd_list[0].flags,
                      cmd_list[0].prdt_length,
                      cmd_list[0].prd_byte_count,
                      cmd_list[0].command_table_base_address,
                      cfis_word,
                      &received_fis.pio[..20],
                      &received_fis.d2h[..20]);
                return None;
            }

            port.log_command_dma_state("identify-complete",
                                       &cmd_list[0],
                                       cmd_table,
                                       received_fis);
            let identify_data = unsafe { &*(data_va as *const ATAIdentifyPacket) };

            debug!("Found ATA Device serial {} firmware {} model {} sectors 24bit={} 48bit={}",
                   from_ata_string(&identify_data.serial).trim_end(),
                   from_ata_string(&identify_data.firmware).trim_end(),
                   from_ata_string(&identify_data.model).trim_end(),
                   identify_data.lba_sectors,
                   identify_data.lba48_sectors);

            let data = unsafe { slice::from_raw_parts_mut(data_va as *mut u8, BLOCK_SIZE) };

            Some(AHCI { header,
                        size,
                        provider : PhantomData,
                        ghc,
                        received_fis,
                        cmd_list,
                        cmd_table,
                        data,
                        port })
        } else {
            None
        }
    }

    pub fn read_block(&mut self, block_id : usize, buf : &mut [u8]) -> usize {
        self.cmd_list[0].flags = REGISTER_H2D_FIS_DWORDS;

        let fis = &mut self.cmd_table.cfis;
        fis.prepare_register_h2d(true);
        // 7.25 READ DMA EXT - 25h, DMA
        fis.command = CMD_READ_DMA_EXT;
        fis.sector_count = 1;
        fis.dev_head |= 0x40; // LBA
        fis.set_lba(block_id as u64);

        self.port
            .issue_command(0);
        if !self.port
                .spin_on_slot(0)
        {
            warn!("AHCI READ DMA EXT timed out");
            return 0;
        }

        let len = buf.len()
                     .min(BLOCK_SIZE);
        buf[..len].clone_from_slice(&self.data[0..len]);
        len
    }

    pub fn write_block(&mut self, block_id : usize, buf : &[u8]) -> usize {
        self.cmd_list[0].flags = REGISTER_H2D_FIS_DWORDS | CommandHeaderFlags::WRITE.bits(); // device write

        let len = buf.len()
                     .min(BLOCK_SIZE);
        self.data[0..len].clone_from_slice(&buf[..len]);

        let fis = &mut self.cmd_table.cfis;
        fis.prepare_register_h2d(true);
        // ATA8-ACS
        // 7.63 WRITE DMA EXT - 35h, DMA
        fis.command = CMD_WRITE_DMA_EXT;
        fis.sector_count = 1;
        fis.dev_head |= 0x40; // LBA
        fis.set_lba(block_id as u64);

        self.port
            .issue_command(0);
        if !self.port
                .spin_on_slot(0)
        {
            warn!("AHCI WRITE DMA EXT timed out");
            return 0;
        }

        len
    }
}

impl<P : Provider> Drop for AHCI<P> {
    fn drop(&mut self) {
        // Command list, RFIS and command table share one control page whose
        // base is the command-list address.
        P::dealloc_dma(self.cmd_list
                           .as_ptr() as usize,
                       P::PAGE_SIZE);
        P::dealloc_dma(self.data.as_ptr() as usize,
                       P::PAGE_SIZE);
    }
}

pub const BLOCK_SIZE : usize = 512;

fn from_ata_string(data : &[u8]) -> String {
    let mut swapped_data = Vec::new();
    assert_eq!(data.len() % 2, 0);
    for i in (0..data.len()).step_by(2) {
        swapped_data.push(data[i + 1]);
        swapped_data.push(data[i]);
    }
    return String::from_utf8(swapped_data).unwrap();
}

#[cfg(test)]
mod tests {
    use core::mem::MaybeUninit;

    use super::*;

    #[test]
    fn received_fis_sentinel_counts_only_device_writes() {
        let mut received_fis = unsafe { MaybeUninit::<AHCIReceivedFIS>::zeroed().assume_init() };

        received_fis.fill_sentinel();
        assert_eq!(received_fis.changed_counts(), (0, 0, 0, 0));

        received_fis.d2h[0] = 0x34;
        received_fis.pio[3] = 0x5A;
        assert_eq!(received_fis.changed_counts(), (0, 1, 1, 0));
    }

    #[test]
    fn softreset_fis_matches_libata_taskfile_defaults() {
        let mut fis = unsafe { MaybeUninit::<SATAFISRegH2D>::zeroed().assume_init() };

        fis.prepare_softreset(true);
        let asserted : [u32; 5] = core::array::from_fn(|index| unsafe {
            core::ptr::read_unaligned((&fis as *const SATAFISRegH2D as *const u32).add(index))
        });
        assert_eq!(asserted, [0x0000_0027,
                              0xA000_0000,
                              0,
                              0x0C00_0000,
                              0]);

        fis.prepare_softreset(false);
        let released : [u32; 5] = core::array::from_fn(|index| unsafe {
            core::ptr::read_unaligned((&fis as *const SATAFISRegH2D as *const u32).add(index))
        });
        assert_eq!(released, [0x0000_0027,
                              0xA000_0000,
                              0,
                              0x0800_0000,
                              0]);
    }

    #[test]
    fn command_fis_keeps_libata_obsolete_bits() {
        let mut fis = unsafe { MaybeUninit::<SATAFISRegH2D>::zeroed().assume_init() };

        fis.prepare_register_h2d(true);
        fis.dev_head |= 0x40;
        assert_eq!(fis.fis_type, FIS_REG_H2D);
        assert_eq!(fis.cflags, 1 << 7);
        assert_eq!(fis.dev_head, ATA_DEVICE_OBS | 0x40);
        assert_eq!(fis.control, ATA_DEVCTL_OBS);
    }
}
