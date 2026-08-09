//! Read-only Loongson-2 MMC pinmux evidence.
//!
//! Linux mainline maps SDIO to bit 20 and the PWM2/GPIO22 card-detect mux to
//! bit 14 of the first pinctrl register. Physical semantics remain
//! `UNVERIFIED_ON_HARDWARE`; normal snapshots never write the register. The
//! isolated transaction API requires explicit unsafe board authority.

use crate::topology::{MmcPinctrlDescription, PinctrlProvider};
use core::{
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

const MUX_REGISTER : usize = 0;
const SDIO_BIT : u32 = 1 << 20;
const CARD_DETECT_GPIO_BIT : u32 = 1 << 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinctrlError {
    Io,
    Missing,
    UnsupportedProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinctrlSnapshot {
    mux_raw : u32,
    sdio_selected : bool,
    card_detect_gpio_selected : bool,
}

impl PinctrlSnapshot {
    pub const fn mux_raw(&self) -> u32 { self.mux_raw }

    pub const fn sdio_selected(&self) -> bool { self.sdio_selected }

    pub const fn card_detect_gpio_selected(&self) -> bool { self.card_detect_gpio_selected }
}

/// Snapshot has been observed but not yet accepted as activation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed;

/// Both upstream-required mux selections were observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ready;

/// At least one required mux selection was not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedsTransition;

/// Typestate proof derived only from a read-only snapshot.
///
/// This proof is instantaneous and does not grant permission to write pinmux
/// registers or remove the board's `PinControlUnavailable` blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinctrlState<S> {
    snapshot : PinctrlSnapshot,
    state : PhantomData<S>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionPlan {
    pub original_raw : u32,
    pub desired_raw : u32,
    pub set_mask : u32,
    pub clear_mask : u32,
}

impl PinctrlState<Observed> {
    pub const fn new(snapshot : PinctrlSnapshot) -> Self {
        Self { snapshot,
               state : PhantomData }
    }

    pub fn classify(self) -> Result<PinctrlState<Ready>, PinctrlState<NeedsTransition>> {
        if self.snapshot
               .mux_raw &
           SDIO_BIT !=
           0 &&
           self.snapshot
               .mux_raw &
           CARD_DETECT_GPIO_BIT ==
           0
        {
            Ok(PinctrlState { snapshot : self.snapshot,
                              state : PhantomData })
        } else {
            Err(PinctrlState { snapshot : self.snapshot,
                               state : PhantomData })
        }
    }
}

impl PinctrlState<Ready> {
    pub const fn snapshot(&self) -> PinctrlSnapshot { self.snapshot }
}

impl PinctrlState<NeedsTransition> {
    pub const fn snapshot(&self) -> PinctrlSnapshot { self.snapshot }

    /// Describe the minimal upstream-derived RMW without performing it.
    pub const fn transition_plan(&self) -> TransitionPlan {
        let set_mask = if self.snapshot
                              .mux_raw &
                          SDIO_BIT !=
                          0
        {
            0
        } else {
            SDIO_BIT
        };
        let clear_mask = if self.snapshot
                                .mux_raw &
                            CARD_DETECT_GPIO_BIT ==
                            0
        {
            0
        } else {
            CARD_DETECT_GPIO_BIT
        };
        TransitionPlan { original_raw : self.snapshot
                                            .mux_raw,
                         desired_raw : (self.snapshot
                                            .mux_raw |
                                        set_mask) &
                                       !clear_mask,
                         set_mask,
                         clear_mask }
    }
}

pub trait RegisterIo {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError>;
}

pub trait WriteRegisterIo: RegisterIo {
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), PinctrlError>;
}

/// Explicit proof that the caller has independently verified this board's
/// pinmux ownership and accepts an `UNVERIFIED_ON_HARDWARE` write.
pub struct TransitionAuthority {
    _private : (),
}

impl TransitionAuthority {
    /// # Safety
    /// The caller must have verified the target board's schematic/DTS, mux
    /// register semantics, exclusive ownership and recovery procedure.
    pub const unsafe fn assume_board_verified() -> Self { Self { _private : () } }
}

struct TransactionGate {
    busy : AtomicBool,
}

impl TransactionGate {
    const fn new() -> Self { Self { busy : AtomicBool::new(false) } }

    pub fn try_enter(&self) -> Result<TransactionGuard<'_>, TransactionBusy> {
        self.busy
            .compare_exchange(false,
                              true,
                              Ordering::AcqRel,
                              Ordering::Acquire)
            .map(|_| TransactionGuard { gate : self })
            .map_err(|_| TransactionBusy)
    }
}

static TRANSACTION_GATE : TransactionGate = TransactionGate::new();

/// Acquire the single WaterOS-local pinctrl transaction gate without waiting.
pub fn try_begin_transition() -> Result<TransactionGuard<'static>, TransactionBusy> {
    TRANSACTION_GATE.try_enter()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransactionBusy;

pub struct TransactionGuard<'a> {
    gate : &'a TransactionGate,
}

impl Drop for TransactionGuard<'_> {
    fn drop(&mut self) {
        self.gate
            .busy
            .store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionStage {
    PreflightRead,
    Write,
    Readback,
    ReadbackMismatch,
    RevalidateRead,
    RevalidateMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionRecovery {
    pub stage : TransitionStage,
    pub attempted_plan : Option<TransitionPlan>,
    pub observed_raw : Option<u32>,
    pub error : Option<PinctrlError>,
}

fn snapshot_from_raw(mux_raw : u32) -> PinctrlSnapshot {
    PinctrlSnapshot { mux_raw,
                      sdio_selected : mux_raw & SDIO_BIT != 0,
                      card_detect_gpio_selected : mux_raw & CARD_DETECT_GPIO_BIT == 0 }
}

/// Re-read, conditionally update and verify the upstream-derived MMC mux.
///
/// The fresh preflight read prevents a stale snapshot from clobbering unrelated
/// bits changed before this guarded transaction. A failed write is treated as
/// having unknown effect and always returns recovery evidence.
pub fn apply_transition(registers : &mut impl WriteRegisterIo,
                        _requested : PinctrlState<NeedsTransition>,
                        _authority : &TransitionAuthority,
                        _guard : &mut TransactionGuard<'_>)
                        -> Result<PinctrlState<Ready>, TransitionRecovery> {
    let current_raw =
        registers.read32(MUX_REGISTER)
                 .map_err(|error| TransitionRecovery { stage : TransitionStage::PreflightRead,
                                                       attempted_plan : None,
                                                       observed_raw : None,
                                                       error : Some(error) })?;
    let current = PinctrlState::<Observed>::new(snapshot_from_raw(current_raw));
    let needed = match current.classify() {
        Ok(ready) => return Ok(ready),
        Err(needed) => needed,
    };
    let plan = needed.transition_plan();
    registers.write32(MUX_REGISTER, plan.desired_raw)
             .map_err(|error| TransitionRecovery { stage : TransitionStage::Write,
                                                   attempted_plan : Some(plan),
                                                   observed_raw : None,
                                                   error : Some(error) })?;
    let observed_raw =
        registers.read32(MUX_REGISTER)
                 .map_err(|error| TransitionRecovery { stage : TransitionStage::Readback,
                                                       attempted_plan : Some(plan),
                                                       observed_raw : None,
                                                       error : Some(error) })?;
    PinctrlState::<Observed>::new(snapshot_from_raw(observed_raw))
        .classify()
        .map_err(|_| TransitionRecovery { stage : TransitionStage::ReadbackMismatch,
                                          attempted_plan : Some(plan),
                                          observed_raw : Some(observed_raw),
                                          error : None })
}

impl TransitionRecovery {
    /// Observe again after an uncertain/failed transaction; never writes.
    pub fn revalidate(&self,
                      registers : &mut impl RegisterIo,
                      _guard : &mut TransactionGuard<'_>)
                      -> Result<PinctrlState<Ready>, TransitionRecovery> {
        let observed_raw = registers.read32(MUX_REGISTER)
                                    .map_err(|error| {
                                        TransitionRecovery { stage:
                                                                 TransitionStage::RevalidateRead,
                                                             attempted_plan : self.attempted_plan,
                                                             observed_raw : None,
                                                             error : Some(error) }
                                    })?;
        PinctrlState::<Observed>::new(snapshot_from_raw(observed_raw))
            .classify()
            .map_err(|_| TransitionRecovery { stage : TransitionStage::RevalidateMismatch,
                                              attempted_plan : self.attempted_plan,
                                              observed_raw : Some(observed_raw),
                                              error : None })
    }
}

pub fn snapshot(state : Option<MmcPinctrlDescription>,
                registers : &mut impl RegisterIo)
                -> Result<PinctrlSnapshot, PinctrlError> {
    match state.map(|state| state.provider) {
        None => Err(PinctrlError::Missing),
        Some(PinctrlProvider::Unsupported) => Err(PinctrlError::UnsupportedProvider),
        Some(PinctrlProvider::Loongson2k { .. }) => {
            let mux_raw = registers.read32(MUX_REGISTER)?;
            Ok(snapshot_from_raw(mux_raw))
        }
    }
}

#[cfg(target_arch = "loongarch64")]
pub struct VolatileRegisters {
    base : usize,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileRegisters {
    /// # Safety
    /// `base..base + size` must be mapped device memory for the pin controller.
    pub unsafe fn new(base : usize, size : usize) -> Result<Self, PinctrlError> {
        if base == 0 || base % 4 != 0 || size < 4 {
            return Err(PinctrlError::Io);
        }
        Ok(Self { base })
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
        if offset != MUX_REGISTER {
            return Err(PinctrlError::Io);
        }
        // SAFETY: constructor and caller uphold the mapped-device-memory contract.
        Ok(unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) })
    }
}

/// Isolated volatile write backend. It is not used by machine init or remote
/// diagnostics and remains `UNVERIFIED_ON_HARDWARE`.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileWriteRegisters {
    base : usize,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileWriteRegisters {
    /// # Safety
    /// The region must be mapped device memory with exclusive pinctrl
    /// ownership, and the caller must provide an independent recovery path.
    pub unsafe fn new(base : usize, size : usize) -> Result<Self, PinctrlError> {
        if base == 0 || base % 4 != 0 || size < 4 {
            return Err(PinctrlError::Io);
        }
        Ok(Self { base })
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileWriteRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
        if offset != MUX_REGISTER {
            return Err(PinctrlError::Io);
        }
        // SAFETY: constructor and caller uphold the mapped-device-memory contract.
        Ok(unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) })
    }
}

#[cfg(target_arch = "loongarch64")]
impl WriteRegisterIo for VolatileWriteRegisters {
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), PinctrlError> {
        if offset != MUX_REGISTER {
            return Err(PinctrlError::Io);
        }
        // SAFETY: constructor and caller uphold the mapped-device-memory contract.
        unsafe { core::ptr::write_volatile((self.base + offset) as *mut u32, value) };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};
    use api_v0::MmioRegion;

    #[derive(Default)]
    struct Model {
        raw : u32,
        reads : usize,
        fail : bool,
    }

    impl RegisterIo for Model {
        fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
            assert_eq!(offset, 0);
            self.reads += 1;
            if self.fail {
                Err(PinctrlError::Io)
            } else {
                Ok(self.raw)
            }
        }
    }

    fn state(provider : PinctrlProvider) -> Option<MmcPinctrlDescription> {
        Some(MmcPinctrlDescription { state_phandle : 1,
                                     provider })
    }

    #[test]
    fn decodes_sdio_and_active_low_gpio_mux_from_one_read() {
        let provider = PinctrlProvider::Loongson2k { mmio : MmioRegion { base : 0x1FE0_0420,
                                                                         size : 0x18 } };
        for (raw, sdio, gpio) in [(1 << 20, true, true),
                                  ((1 << 20) | (1 << 14), true, false),
                                  (0, false, true)]
        {
            let mut registers = Model { raw,
                                        ..Default::default() };
            let result = snapshot(state(provider), &mut registers).unwrap();
            assert_eq!((result.sdio_selected, result.card_detect_gpio_selected),
                       (sdio, gpio));
            assert_eq!(registers.reads, 1);
        }
    }

    #[test]
    fn missing_and_unsupported_never_read_and_io_error_is_retained() {
        let mut registers = Model::default();
        assert_eq!(snapshot(None, &mut registers),
                   Err(PinctrlError::Missing));
        assert_eq!(snapshot(state(PinctrlProvider::Unsupported),
                            &mut registers),
                   Err(PinctrlError::UnsupportedProvider));
        assert_eq!(registers.reads, 0);
        registers.fail = true;
        assert_eq!(snapshot(state(PinctrlProvider::Loongson2k { mmio:
                                                                    MmioRegion { base : 4,
                                                                                 size : 0x18 } }),
                            &mut registers),
                   Err(PinctrlError::Io));
    }

    #[test]
    fn only_a_fully_selected_snapshot_produces_a_ready_token() {
        let ready = PinctrlState::<Observed>::new(PinctrlSnapshot {
            mux_raw : SDIO_BIT,
            sdio_selected : true,
            card_detect_gpio_selected : true,
        }).classify().expect("ready pinmux token");
        assert_eq!(ready.snapshot()
                        .mux_raw(),
                   SDIO_BIT);

        for snapshot in [PinctrlSnapshot { mux_raw : 0,
                                           sdio_selected : false,
                                           card_detect_gpio_selected : true },
                         PinctrlSnapshot { mux_raw : SDIO_BIT | CARD_DETECT_GPIO_BIT,
                                           sdio_selected : true,
                                           card_detect_gpio_selected : false }]
        {
            assert!(PinctrlState::<Observed>::new(snapshot).classify()
                                                           .is_err());
        }
    }

    #[test]
    fn transition_plan_changes_only_the_two_upstream_mux_bits() {
        let unrelated = (1 << 3) | (1 << 27);
        let snapshot = PinctrlSnapshot { mux_raw : unrelated | CARD_DETECT_GPIO_BIT,
                                         sdio_selected : false,
                                         card_detect_gpio_selected : false };
        let needed = PinctrlState::<Observed>::new(snapshot).classify()
                                                            .expect_err("transition required");
        let plan = needed.transition_plan();
        assert_eq!(plan.set_mask, SDIO_BIT);
        assert_eq!(plan.clear_mask, CARD_DETECT_GPIO_BIT);
        assert_eq!(plan.desired_raw, unrelated | SDIO_BIT);
        assert_eq!((plan.original_raw ^ plan.desired_raw) & !(SDIO_BIT | CARD_DETECT_GPIO_BIT),
                   0);
    }

    struct TransactionModel {
        read_values : Vec<u32>,
        next_read : usize,
        writes : Vec<(usize, u32)>,
        fail_write : bool,
    }

    impl RegisterIo for TransactionModel {
        fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
            assert_eq!(offset, MUX_REGISTER);
            let value = self.read_values
                            .get(self.next_read)
                            .copied()
                            .ok_or(PinctrlError::Io)?;
            self.next_read += 1;
            Ok(value)
        }
    }

    impl WriteRegisterIo for TransactionModel {
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), PinctrlError> {
            self.writes
                .push((offset, value));
            if self.fail_write {
                Err(PinctrlError::Io)
            } else {
                Ok(())
            }
        }
    }

    fn requested_transition() -> PinctrlState<NeedsTransition> {
        PinctrlState::<Observed>::new(snapshot_from_raw(0)).classify()
                                                           .expect_err("transition request")
    }

    fn authority() -> TransitionAuthority {
        // SAFETY: mock registers have no physical board side effects.
        unsafe { TransitionAuthority::assume_board_verified() }
    }

    #[test]
    fn transaction_gate_rejects_reentry_and_reopens_after_drop() {
        let guard = try_begin_transition().unwrap();
        assert!(matches!(try_begin_transition(),
                         Err(TransactionBusy)));
        drop(guard);
        assert!(try_begin_transition().is_ok());
    }

    #[test]
    fn transaction_rereads_preserves_current_unrelated_bits_and_verifies() {
        let unrelated = (1 << 3) | (1 << 27);
        let mut registers = TransactionModel { read_values : vec![unrelated |
                                                                  CARD_DETECT_GPIO_BIT,
                                                                  unrelated | SDIO_BIT],
                                               next_read : 0,
                                               writes : Vec::new(),
                                               fail_write : false };
        let gate = TransactionGate::new();
        let mut guard = gate.try_enter()
                            .unwrap();
        let ready = apply_transition(&mut registers,
                                     requested_transition(),
                                     &authority(),
                                     &mut guard).unwrap();
        assert_eq!(ready.snapshot()
                        .mux_raw(),
                   unrelated | SDIO_BIT);
        assert_eq!(registers.writes, vec![(MUX_REGISTER,
                                           unrelated |
                                           SDIO_BIT)]);
        assert_eq!(registers.next_read, 2);

        let mut already_ready = TransactionModel { read_values : vec![unrelated | SDIO_BIT],
                                                   next_read : 0,
                                                   writes : Vec::new(),
                                                   fail_write : false };
        let ready = apply_transition(&mut already_ready,
                                     requested_transition(),
                                     &authority(),
                                     &mut guard).unwrap();
        assert_eq!(ready.snapshot()
                        .mux_raw(),
                   unrelated | SDIO_BIT);
        assert!(already_ready.writes
                             .is_empty());
        assert_eq!(already_ready.next_read, 1);
    }

    #[test]
    fn uncertain_write_and_mismatch_remain_revalidatable() {
        let gate = TransactionGate::new();
        let mut guard = gate.try_enter()
                            .unwrap();
        let mut preflight_failure = TransactionModel { read_values : Vec::new(),
                                                       next_read : 0,
                                                       writes : Vec::new(),
                                                       fail_write : false };
        let recovery = apply_transition(&mut preflight_failure,
                                        requested_transition(),
                                        &authority(),
                                        &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   TransitionStage::PreflightRead);
        assert_eq!(recovery.attempted_plan, None);

        let mut failed_write = TransactionModel { read_values : vec![0, SDIO_BIT],
                                                  next_read : 0,
                                                  writes : Vec::new(),
                                                  fail_write : true };
        let recovery = apply_transition(&mut failed_write,
                                        requested_transition(),
                                        &authority(),
                                        &mut guard).unwrap_err();
        assert_eq!(recovery.stage, TransitionStage::Write);
        assert_eq!(recovery.observed_raw, None);
        assert_eq!(recovery.revalidate(&mut failed_write, &mut guard)
                           .unwrap()
                           .snapshot()
                           .mux_raw(),
                   SDIO_BIT);

        let mut failed_readback = TransactionModel { read_values : vec![0],
                                                     next_read : 0,
                                                     writes : Vec::new(),
                                                     fail_write : false };
        let recovery = apply_transition(&mut failed_readback,
                                        requested_transition(),
                                        &authority(),
                                        &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   TransitionStage::Readback);
        assert!(recovery.attempted_plan
                        .is_some());

        let mut mismatch = TransactionModel { read_values : vec![0, 0, SDIO_BIT],
                                              next_read : 0,
                                              writes : Vec::new(),
                                              fail_write : false };
        let recovery = apply_transition(&mut mismatch,
                                        requested_transition(),
                                        &authority(),
                                        &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   TransitionStage::ReadbackMismatch);
        assert_eq!(recovery.observed_raw, Some(0));
        assert_eq!(recovery.revalidate(&mut mismatch, &mut guard)
                           .unwrap()
                           .snapshot()
                           .mux_raw(),
                   SDIO_BIT);
    }
}
