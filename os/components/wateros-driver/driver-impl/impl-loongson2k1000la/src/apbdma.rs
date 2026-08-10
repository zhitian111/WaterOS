//! Pure-data Loongson-2 APBDMA descriptor planning.
//!
//! This module performs no allocation, address translation, cache maintenance
//! or MMIO. A future executor must supply DMA-capable physical memory and the
//! architecture-specific cache operations before using a plan on hardware.
use crate::topology::DmaControllerDescription;
use crate::dma_memory::DmaAllocationError;
use crate::irq_domain::{AcknowledgedIrq, GlobalIrq};
#[cfg(target_arch = "loongarch64")]
use crate::dma_memory::OwnedDmaBuffer;
use alloc::vec::Vec;
use api_v0::{DriverError, DriverResult,
             dma::{DmaCoherency, DmaDirection, DmaMapping, DmaRegion}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    DeviceToMemory,
    MemoryToDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    EmptyTransfer,
    UnalignedLength,
    UnalignedDescriptor,
    InvalidBurst,
    AddressOutOfRange,
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLease {
    provider_phandle : u32,
    channel : u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseError {
    UnknownProvider,
    InvalidChannel,
    Busy,
    NotOwned,
}

pub struct ChannelLeases {
    slots : Vec<(u32, bool)>,
}

impl ChannelLeases {
    pub fn from_topology(controllers : &[DmaControllerDescription]) -> Self {
        Self { slots : controllers.iter().map(|controller| (controller.phandle, false)).collect() }
    }

    pub fn claim(&mut self, provider_phandle : u32, channel : u32)
                 -> Result<ChannelLease, LeaseError> {
        if channel != 0 { return Err(LeaseError::InvalidChannel); }
        let slot = self.slots.iter_mut()
                             .find(|(provider, _)| *provider == provider_phandle)
                             .ok_or(LeaseError::UnknownProvider)?;
        if slot.1 { return Err(LeaseError::Busy); }
        slot.1 = true;
        Ok(ChannelLease { provider_phandle, channel })
    }

    pub fn release(&mut self, lease : ChannelLease) -> Result<(), LeaseError> {
        let slot = self.slots.iter_mut()
                             .find(|(provider, _)| *provider == lease.provider_phandle)
                             .ok_or(LeaseError::UnknownProvider)?;
        if lease.channel != 0 || !slot.1 { return Err(LeaseError::NotOwned); }
        slot.1 = false;
        Ok(())
    }
}

/// Hardware layout used by the upstream Loongson-2 APBDMA driver.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HardwareDescriptor {
    pub next_address_low : u32,
    pub memory_address_low : u32,
    pub apb_address : u32,
    pub length_words : u32,
    pub step_length : u32,
    pub step_times : u32,
    pub command : u32,
    pub status : u32,
    pub next_address_high : u32,
    pub memory_address_high : u32,
    pub reserved : [u32; 2],
}

const COMMAND_INTERRUPT : u32 = 1 << 1;
const COMMAND_MEMORY_TO_DEVICE : u32 = 1 << 12;
const ORDER_64_BIT : u64 = 1 << 0;
const ORDER_START : u64 = 1 << 3;
const ORDER_STOP : u64 = 1 << 4;
const ORDER_CONFIG_MASK : u64 = 0x1f;
const DEFAULT_STOP_POLL_LIMIT : usize = 1024;
const DMA_ROUTE_MASK : u32 = 0b111 << 15;
const DMA1_ROUTE : u32 = 1 << 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferPlan {
    pub descriptor : HardwareDescriptor,
    pub descriptor_physical_address : u64,
    pub start_order : u64,
    pub memory_physical_address : u64,
    pub byte_length : usize,
    pub direction : Direction,
    /// The buffer must be invalidated after a device-to-memory transfer.
    pub invalidate_buffer_after : bool,
    /// Descriptor contents must be visible before the order register is written.
    pub clean_descriptor_before : bool,
}

#[derive(Debug)]
pub struct PreparedTransfer(TransferPlan);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceError {
    Allocation(DmaAllocationError),
    InvalidPlan(PlanError),
    Driver(DriverError),
}

impl From<DmaAllocationError> for ResourceError {
    fn from(error : DmaAllocationError) -> Self { Self::Allocation(error) }
}

impl From<PlanError> for ResourceError {
    fn from(error : PlanError) -> Self { Self::InvalidPlan(error) }
}

impl From<DriverError> for ResourceError {
    fn from(error : DriverError) -> Self { Self::Driver(error) }
}

/// Descriptor and payload allocations whose lifetime covers one APBDMA transfer.
#[cfg(target_arch = "loongarch64")]
pub struct OwnedTransferResources<D, P> {
    plan : TransferPlan,
    descriptor : OwnedDmaBuffer<D>,
    payload : OwnedDmaBuffer<P>,
}

#[cfg(target_arch = "loongarch64")]
impl<D : DmaCoherency, P : DmaCoherency> OwnedTransferResources<D, P> {
    /// Allocate real contiguous RAM and encode its physical addresses into the
    /// descriptor. Supplying a production coherency backend remains mandatory.
    pub fn allocate(apb_address : u64,
                    byte_length : usize,
                    burst_words : u32,
                    direction : Direction,
                    device_address_bits : u8,
                    descriptor_coherency : D,
                    payload_coherency : P)
                    -> Result<Self, ResourceError> {
        let mut descriptor = OwnedDmaBuffer::allocate_zeroed(
            core::mem::size_of::<HardwareDescriptor>(),
            32,
            device_address_bits,
            DmaDirection::Bidirectional,
            descriptor_coherency)?;
        let payload = OwnedDmaBuffer::allocate_zeroed(byte_length,
                                                      4,
                                                      device_address_bits,
                                                      dma_direction(direction),
                                                      payload_coherency)?;
        let descriptor_region = descriptor.region()?;
        let payload_region = payload.region()?;
        let plan = build_transfer(descriptor_region.physical_address(),
                                  payload_region.physical_address(),
                                  apb_address,
                                  byte_length,
                                  burst_words,
                                  direction)?;
        let descriptor_bytes = unsafe {
            core::slice::from_raw_parts(
                (&plan.descriptor as *const HardwareDescriptor).cast::<u8>(),
                core::mem::size_of::<HardwareDescriptor>())
        };
        descriptor.cpu_bytes_mut()?.copy_from_slice(descriptor_bytes);
        Ok(Self { plan, descriptor, payload })
    }

    pub const fn plan(&self) -> TransferPlan { self.plan }

    pub fn payload_bytes(&self) -> DriverResult<&[u8]> { self.payload.cpu_bytes() }

    pub fn payload_bytes_mut(&mut self) -> DriverResult<&mut [u8]> {
        self.payload.cpu_bytes_mut()
    }

    pub fn prepare_session(&mut self) -> DriverResult<PreparedSession<'_, D, P>> {
        prepare_session(self.plan,
                        self.descriptor.mapping_mut(),
                        self.payload.mapping_mut())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorError {
    Busy,
    Idle,
    Register,
    InvalidPollLimit,
    StopTimeout,
    StopUnverified,
    UnexpectedIrq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteEffect {
    Untouched,
    MayHaveWritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderWriteFailure {
    pub error : ExecutorError,
    pub effect : WriteEffect,
}

#[derive(Debug)]
pub struct StartFailure {
    pub error : ExecutorError,
    pub prepared : PreparedTransfer,
    recovery_required : bool,
}

pub trait OrderIo {
    fn read64(&mut self) -> Result<u64, ExecutorError>;
    fn write64(&mut self, value : u64) -> Result<(), OrderWriteFailure>;
    /// Report whether hardware is proven to have stopped. Implementations must
    /// return `StopUnverified` when the platform has no documented probe.
    fn confirm_stopped(&mut self) -> Result<bool, ExecutorError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    plan : TransferPlan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorCompletion {
    Complete,
    HardwareError(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorStatusError {
    Cache(DriverError),
    Read,
    StatusUnverified,
    Unknown(u32),
}

pub trait DescriptorStatusReader {
    fn read_status(&mut self, descriptor : api_v0::dma::DmaRegion)
                   -> Result<u32, DescriptorStatusError>;
}

pub trait DescriptorStatusDecoder {
    fn decode(&self, status : u32) -> Result<DescriptorCompletion, DescriptorStatusError>;
}

pub struct UnverifiedStatusDecoder;

impl DescriptorStatusDecoder for UnverifiedStatusDecoder {
    fn decode(&self, _status : u32) -> Result<DescriptorCompletion, DescriptorStatusError> {
        Err(DescriptorStatusError::StatusUnverified)
    }
}

/// Volatile descriptor status access after descriptor cache ownership has
/// already returned to the CPU.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileDescriptorStatusReader;

#[cfg(target_arch = "loongarch64")]
impl DescriptorStatusReader for VolatileDescriptorStatusReader {
    fn read_status(&mut self, descriptor : api_v0::dma::DmaRegion)
                   -> Result<u32, DescriptorStatusError> {
        let offset = core::mem::offset_of!(HardwareDescriptor, status);
        if descriptor.length() < offset + core::mem::size_of::<u32>() {
            return Err(DescriptorStatusError::Read);
        }
        let address = descriptor.virtual_address()
                                .checked_add(offset)
                                .ok_or(DescriptorStatusError::Read)?;
        Ok(unsafe { core::ptr::read_volatile(address as *const u32) })
    }
}

impl Completion {
    pub const fn requires_buffer_invalidate(self) -> bool { self.plan.invalidate_buffer_after }

    pub const fn plan(self) -> TransferPlan { self.plan }
}

/// A failed state transition that returns the original session for retry or
/// explicit cancellation.
pub struct SessionFailure<E, S> {
    pub error : E,
    pub session : S,
}

impl<E : core::fmt::Debug, S> core::fmt::Debug for SessionFailure<E, S> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("SessionFailure")
                 .field("error", &self.error)
                 .finish_non_exhaustive()
    }
}

/// IRQ completion failure retaining both the linear acknowledgement and the
/// still-running session for validation, retry or explicit stop recovery.
pub struct IrqSessionFailure<E, S> {
    pub error : E,
    pub acknowledged : AcknowledgedIrq,
    pub session : S,
}

impl<E : core::fmt::Debug, S> core::fmt::Debug for IrqSessionFailure<E, S> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("IrqSessionFailure")
                 .field("error", &self.error)
                 .field("acknowledged", &self.acknowledged)
                 .finish_non_exhaustive()
    }
}

/// Both mappings have been synchronized for the device but hardware has not
/// started. The mutable borrows keep the backing resources alive.
#[must_use = "cancel or start a prepared DMA session"]
pub struct PreparedSession<'a, D, P> {
    prepared : PreparedTransfer,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// Hardware is running and exclusively borrows both mappings and the executor.
#[must_use = "complete the IRQ or stop a running DMA session"]
pub struct RunningSession<'a, 'e, R, D, P> {
    executor : &'e mut Executor<R>,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// The expected APBDMA IRQ was masked/acknowledged and hardware is no longer
/// treated as running. Descriptor visibility/status inspection remains.
#[must_use = "inspect descriptor status or explicitly reclaim an IRQ completion"]
pub struct IrqCompletionSession<'a, D, P> {
    completion : Completion,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// A start-register write may have reached hardware. The mappings remain
/// device-owned until an explicit stop confirms the channel is quiescent.
#[must_use = "stop a DMA session whose start state is uncertain"]
pub struct RecoverySession<'a, 'e, R, D, P> {
    executor : &'e mut Executor<R>,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

pub enum StartSessionFailure<'a, 'e, R, D, P> {
    Prepared(SessionFailure<ExecutorError, PreparedSession<'a, D, P>>),
    Recovery(SessionFailure<ExecutorError, RecoverySession<'a, 'e, R, D, P>>),
}

impl<R, D, P> core::fmt::Debug for StartSessionFailure<'_, '_, R, D, P> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (state, error) = match self {
            Self::Prepared(failure) => ("Prepared", &failure.error),
            Self::Recovery(failure) => ("Recovery", &failure.error),
        };
        formatter.debug_struct("StartSessionFailure")
                 .field("state", &state)
                 .field("error", error)
                 .finish()
    }
}

/// Hardware is confirmed idle; CPU-side cache synchronization remains.
#[must_use = "finish cache synchronization for a quiesced DMA session"]
pub struct QuiescedSession<'a, D, P> {
    completion : Completion,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

/// One-shot handoff proving APBDMA stop confirmation while retaining both
/// device-owned mappings until explicit cache synchronization.
#[must_use = "finish or retain a quiesced DMA handoff"]
pub struct QuiescedHandoff<'a, D, P> {
    completion : Completion,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuiescedHandoffIdentity {
    pub transfer : TransferPlan,
    pub descriptor_region : DmaRegion,
    pub descriptor_direction : DmaDirection,
    pub payload_region : DmaRegion,
    pub payload_direction : DmaDirection,
}

/// Both mappings have completed their CPU-side synchronization. Consuming this
/// value is the only handoff path that releases the mutable mapping borrows.
#[must_use = "release the CPU-owned DMA mappings"]
pub struct CpuOwnedHandoff<'a, D, P> {
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>,
}

impl<'a, D : DmaCoherency, P : DmaCoherency> PreparedSession<'a, D, P> {
    pub fn cancel(self) -> Result<(), SessionFailure<DriverError, Self>> {
        match cancel_prepared(&self.prepared, self.descriptor, self.payload) {
            Ok(()) => Ok(()),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }

    pub fn start<'e, R : OrderIo>(self,
                                  executor : &'e mut Executor<R>)
                                  -> Result<RunningSession<'a, 'e, R, D, P>,
                                            StartSessionFailure<'a, 'e, R, D, P>> {
        match executor.start(self.prepared) {
            Ok(()) => Ok(RunningSession { executor,
                                         descriptor : self.descriptor,
                                         payload : self.payload }),
            Err(failure) if failure.recovery_required => {
                Err(StartSessionFailure::Recovery(SessionFailure {
                    error : failure.error,
                    session : RecoverySession { executor,
                                                descriptor : self.descriptor,
                                                payload : self.payload },
                }))
            },
            Err(failure) => Err(StartSessionFailure::Prepared(SessionFailure {
                error : failure.error,
                session : PreparedSession { prepared : failure.prepared,
                                            descriptor : self.descriptor,
                                            payload : self.payload },
            })),
        }
    }
}

impl<'a, 'e, R : OrderIo, D, P> RunningSession<'a, 'e, R, D, P> {
    pub fn complete_irq(self, acknowledged : AcknowledgedIrq)
                        -> Result<IrqCompletionSession<'a, D, P>,
                                  IrqSessionFailure<ExecutorError, Self>> {
        match self.executor.complete_irq(acknowledged) {
            Ok(completion) => Ok(IrqCompletionSession { completion,
                                                       descriptor : self.descriptor,
                                                       payload : self.payload }),
            Err((error, acknowledged)) => Err(IrqSessionFailure {
                error, acknowledged, session : self,
            }),
        }
    }

    pub fn stop(self)
                -> Result<QuiescedSession<'a, D, P>,
                          SessionFailure<ExecutorError, Self>> {
        match self.executor.stop() {
            Ok(completion) => Ok(QuiescedSession { completion,
                                                  descriptor : self.descriptor,
                                                  payload : self.payload }),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }
}

impl<'a, D : DmaCoherency, P> IrqCompletionSession<'a, D, P> {
    pub fn inspect_status<R : DescriptorStatusReader, C : DescriptorStatusDecoder>(
        self,
        reader : &mut R,
        decoder : &C)
        -> Result<(DescriptorCompletion, QuiescedSession<'a, D, P>),
                  SessionFailure<DescriptorStatusError, Self>> {
        let descriptor = if self.descriptor.is_cpu_owned() {
            self.descriptor.cpu_region()
        } else {
            self.descriptor.complete_from_device()
        };
        let descriptor = match descriptor {
            Ok(descriptor) => descriptor,
            Err(error) => {
                return Err(SessionFailure { error : DescriptorStatusError::Cache(error),
                                            session : self });
            },
        };
        let status = match reader.read_status(descriptor) {
            Ok(status) => status,
            Err(error) => return Err(SessionFailure { error, session : self }),
        };
        let result = match decoder.decode(status) {
            Ok(result) => result,
            Err(error) => return Err(SessionFailure { error, session : self }),
        };
        Ok((result,
            QuiescedSession { completion : self.completion,
                              descriptor : self.descriptor,
                              payload : self.payload }))
    }

    /// Reclaim resources after an expected, acknowledged IRQ without claiming
    /// that the undocumented descriptor status represents success.
    ///
    /// # Safety
    /// The caller must have platform evidence that this IRQ means the DMA
    /// engine has stopped accessing both mappings.
    pub unsafe fn reclaim_unverified(self) -> QuiescedSession<'a, D, P> {
        QuiescedSession { completion : self.completion,
                          descriptor : self.descriptor,
                          payload : self.payload }
    }
}

impl<'a, 'e, R : OrderIo, D, P> RecoverySession<'a, 'e, R, D, P> {
    pub fn stop(self)
                -> Result<QuiescedSession<'a, D, P>,
                          SessionFailure<ExecutorError, Self>> {
        match self.executor.stop() {
            Ok(completion) => Ok(QuiescedSession { completion,
                                                  descriptor : self.descriptor,
                                                  payload : self.payload }),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }
}

impl<'a, D : DmaCoherency, P : DmaCoherency> QuiescedSession<'a, D, P> {
    pub fn finish(self) -> Result<(), SessionFailure<DriverError, Self>> {
        match finish_transfer(self.completion, self.descriptor, self.payload) {
            Ok(()) => Ok(()),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }

    pub fn into_handoff(self) -> QuiescedHandoff<'a, D, P> {
        QuiescedHandoff { completion : self.completion,
                          descriptor : self.descriptor,
                          payload : self.payload }
    }
}

impl<'a, D : DmaCoherency, P : DmaCoherency> QuiescedHandoff<'a, D, P> {
    pub fn identity(&self) -> QuiescedHandoffIdentity {
        QuiescedHandoffIdentity { transfer : self.completion.plan(),
                                  descriptor_region : self.descriptor.identity_region(),
                                  descriptor_direction : self.descriptor.identity_direction(),
                                  payload_region : self.payload.identity_region(),
                                  payload_direction : self.payload.identity_direction() }
    }

    pub fn finish(self)
                  -> Result<CpuOwnedHandoff<'a, D, P>,
                            SessionFailure<DriverError, Self>> {
        match finish_transfer(self.completion, self.descriptor, self.payload) {
            Ok(()) => Ok(CpuOwnedHandoff { descriptor : self.descriptor,
                                           payload : self.payload }),
            Err(error) => Err(SessionFailure { error, session : self }),
        }
    }
}

impl<'a, D, P> CpuOwnedHandoff<'a, D, P> {
    pub fn into_mappings(self) -> (&'a mut DmaMapping<D>, &'a mut DmaMapping<P>) {
        (self.descriptor, self.payload)
    }
}

fn dma_direction(direction : Direction) -> DmaDirection {
    match direction {
        Direction::DeviceToMemory => DmaDirection::FromDevice,
        Direction::MemoryToDevice => DmaDirection::ToDevice,
    }
}

/// Validate actual mappings against the plan and transfer both to the device.
/// Safe code cannot construct [`PreparedTransfer`] by assertion alone.
pub(crate) fn prepare_transfer<D : DmaCoherency, P : DmaCoherency>(
    plan : TransferPlan,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<PreparedTransfer> {
    let words = plan.byte_length / 4;
    let descriptor_words = (plan.descriptor.length_words as usize)
                                .checked_mul(plan.descriptor.step_times as usize);
    let expected_command = COMMAND_INTERRUPT |
        if plan.direction == Direction::MemoryToDevice { COMMAND_MEMORY_TO_DEVICE } else { 0 };
    if plan.byte_length == 0 || plan.byte_length % 4 != 0 ||
       plan.descriptor.memory_address_low != plan.memory_physical_address as u32 ||
       plan.descriptor.memory_address_high != (plan.memory_physical_address >> 32) as u32 ||
       plan.descriptor.command != expected_command ||
       plan.invalidate_buffer_after != (plan.direction == Direction::DeviceToMemory) ||
       !plan.clean_descriptor_before ||
       plan.start_order != plan.descriptor_physical_address | ORDER_64_BIT | ORDER_START ||
       descriptor_words.is_none_or(|encoded| encoded < words)
    {
        return Err(DriverError::InvalidParam);
    }
    let descriptor_region = descriptor.cpu_region()?;
    let payload_region = payload.cpu_region()?;
    if descriptor.direction() != DmaDirection::Bidirectional ||
       descriptor_region.physical_address() != plan.descriptor_physical_address ||
       descriptor_region.length() < core::mem::size_of::<HardwareDescriptor>() ||
       payload.direction() != dma_direction(plan.direction) ||
       payload_region.physical_address() != plan.memory_physical_address ||
       payload_region.length() != plan.byte_length
    {
        return Err(DriverError::InvalidParam);
    }
    descriptor.prepare_for_device()?;
    if let Err(error) = payload.prepare_for_device() {
        descriptor.reclaim_after_stop().map_err(|_| DriverError::IoError)?;
        return Err(error);
    }
    Ok(PreparedTransfer(plan))
}

pub fn prepare_session<'a, D : DmaCoherency, P : DmaCoherency>(
    plan : TransferPlan,
    descriptor : &'a mut DmaMapping<D>,
    payload : &'a mut DmaMapping<P>)
    -> DriverResult<PreparedSession<'a, D, P>> {
    let prepared = prepare_transfer(plan, descriptor, payload)?;
    Ok(PreparedSession { prepared, descriptor, payload })
}

pub(crate) fn finish_transfer<D : DmaCoherency, P : DmaCoherency>(
    _completion : Completion,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<()> {
    let payload_result = if payload.is_cpu_owned() {
        Ok(())
    } else {
        payload.complete_from_device().map(|_| ())
    };
    let descriptor_result = if descriptor.is_cpu_owned() {
        Ok(())
    } else {
        descriptor.complete_from_device().map(|_| ())
    };
    if payload.is_cpu_owned() && descriptor.is_cpu_owned() { return Ok(()); }
    payload_result?;
    descriptor_result?;
    Ok(())
}

pub(crate) fn cancel_prepared<D : DmaCoherency, P : DmaCoherency>(
    _prepared : &PreparedTransfer,
    descriptor : &mut DmaMapping<D>,
    payload : &mut DmaMapping<P>)
    -> DriverResult<()> {
    let payload_result = if payload.is_cpu_owned() {
        Ok(())
    } else {
        payload.reclaim_after_stop().map(|_| ())
    };
    let descriptor_result = if descriptor.is_cpu_owned() {
        Ok(())
    } else {
        descriptor.reclaim_after_stop().map(|_| ())
    };
    if payload.is_cpu_owned() && descriptor.is_cpu_owned() { return Ok(()); }
    payload_result?;
    descriptor_result?;
    Ok(())
}

pub struct Executor<R> {
    registers : R,
    running : Option<TransferPlan>,
    stop_poll_limit : usize,
    expected_irq : GlobalIrq,
}

impl<R : OrderIo> Executor<R> {
    pub fn new(registers : R, expected_irq : GlobalIrq) -> Self {
        Self { registers,
               running : None,
               stop_poll_limit : DEFAULT_STOP_POLL_LIMIT,
               expected_irq }
    }

    pub fn with_stop_poll_limit(registers : R,
                                expected_irq : GlobalIrq,
                                stop_poll_limit : usize)
                                -> Result<Self, ExecutorError> {
        if stop_poll_limit == 0 { return Err(ExecutorError::InvalidPollLimit); }
        Ok(Self { registers, running : None, stop_poll_limit, expected_irq })
    }

    pub(crate) fn start(&mut self, prepared : PreparedTransfer) -> Result<(), StartFailure> {
        if self.running.is_some() {
            return Err(StartFailure { error : ExecutorError::Busy,
                                      prepared,
                                      recovery_required : false });
        }
        if let Err(failure) = self.registers.write64(0) {
            if failure.effect == WriteEffect::MayHaveWritten {
                self.running = Some(prepared.0);
            }
            return Err(StartFailure { error : failure.error,
                                      prepared,
                                      recovery_required : failure.effect ==
                                                          WriteEffect::MayHaveWritten });
        }
        if let Err(failure) = self.registers.write64(prepared.0.start_order) {
            if failure.effect == WriteEffect::MayHaveWritten {
                self.running = Some(prepared.0);
            }
            return Err(StartFailure { error : failure.error,
                                      prepared,
                                      recovery_required : failure.effect ==
                                                          WriteEffect::MayHaveWritten });
        }
        self.running = Some(prepared.0);
        Ok(())
    }

    /// Called only after the APBDMA IRQ has been claimed and acknowledged.
    pub(crate) fn complete_irq(&mut self, acknowledged : AcknowledgedIrq)
                               -> Result<Completion, (ExecutorError, AcknowledgedIrq)> {
        if acknowledged.irq() != self.expected_irq {
            return Err((ExecutorError::UnexpectedIrq, acknowledged));
        }
        let plan = match self.running.take() {
            Some(plan) => plan,
            None => return Err((ExecutorError::Idle, acknowledged)),
        };
        Ok(Completion { plan })
    }

    pub(crate) fn stop(&mut self) -> Result<Completion, ExecutorError> {
        let plan = self.running.ok_or(ExecutorError::Idle)?;
        let current = self.registers.read64()?;
        self.registers.write64((current & !ORDER_CONFIG_MASK) | ORDER_64_BIT | ORDER_STOP)
                      .map_err(|failure| failure.error)?;
        for _ in 0..self.stop_poll_limit {
            if self.registers.confirm_stopped()? {
                self.running = None;
                return Ok(Completion { plan });
            }
        }
        Err(ExecutorError::StopTimeout)
    }

    #[cfg(test)]
    fn into_inner(self) -> R { self.registers }
}

/// Preserve unrelated system-register bits and route SDIO to APBDMA1.
pub const fn route_sdio_to_dma1(current : u32) -> u32 {
    (current & !DMA_ROUTE_MASK) | DMA1_ROUTE
}

pub fn build_transfer(descriptor_physical_address : u64,
                      memory_physical_address : u64,
                      apb_address : u64,
                      byte_length : usize,
                      burst_words : u32,
                      direction : Direction)
                      -> Result<TransferPlan, PlanError> {
    if byte_length == 0 { return Err(PlanError::EmptyTransfer); }
    if byte_length % 4 != 0 { return Err(PlanError::UnalignedLength); }
    if descriptor_physical_address & ORDER_CONFIG_MASK != 0 {
        return Err(PlanError::UnalignedDescriptor);
    }
    if burst_words == 0 { return Err(PlanError::InvalidBurst); }
    let apb_address = u32::try_from(apb_address).map_err(|_| PlanError::AddressOutOfRange)?;
    let words = u32::try_from(byte_length / 4).map_err(|_| PlanError::ArithmeticOverflow)?;
    let step_times = words.div_ceil(burst_words);
    let length_words = words.div_ceil(step_times);
    let command = COMMAND_INTERRUPT |
                  if direction == Direction::MemoryToDevice {
                      COMMAND_MEMORY_TO_DEVICE
                  } else {
                      0
                  };
    let descriptor = HardwareDescriptor {
        memory_address_low : memory_physical_address as u32,
        memory_address_high : (memory_physical_address >> 32) as u32,
        apb_address,
        length_words,
        step_times,
        command,
        ..HardwareDescriptor::default()
    };
    Ok(TransferPlan {
        descriptor,
        descriptor_physical_address,
        start_order : (descriptor_physical_address & !ORDER_CONFIG_MASK) |
                      ORDER_64_BIT | ORDER_START,
        memory_physical_address,
        byte_length,
        direction,
        invalidate_buffer_after : direction == Direction::DeviceToMemory,
        clean_descriptor_before : true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use api_v0::dma::{DmaRegion, DmaCoherency};
    use crate::irq_owner::IrqOwner;

    fn dma_irq() -> GlobalIrq { GlobalIrq::from_bank_local(1, 13).unwrap() }
    fn acknowledged_dma_irq() -> AcknowledgedIrq {
        AcknowledgedIrq::after_mask_ack(dma_irq())
    }
    fn armed_read_irqs(raw : u64) -> crate::board_irq_owner::ArmedReadIrqs {
        crate::board_irq_owner::ArmedReadIrqs::fixture(
            crate::board_irq_owner::ReadTransactionId::new(raw).unwrap())
    }

    #[test]
    fn builds_64_bit_read_descriptor_and_start_order() {
        let plan = build_transfer(0x1_0000_0040,
                                  0x2_1234_5000,
                                  0x1fe2_c040,
                                  512,
                                  16,
                                  Direction::DeviceToMemory).unwrap();
        assert_eq!(core::mem::size_of::<HardwareDescriptor>(), 48);
        assert_eq!(plan.descriptor.memory_address_low, 0x1234_5000);
        assert_eq!(plan.descriptor.memory_address_high, 2);
        assert_eq!(plan.descriptor.apb_address, 0x1fe2_c040);
        assert_eq!(plan.descriptor.length_words, 16);
        assert_eq!(plan.descriptor.step_times, 8);
        assert_eq!(plan.descriptor.command, COMMAND_INTERRUPT);
        assert_eq!(plan.start_order, 0x1_0000_0049);
        assert!(plan.invalidate_buffer_after);
        assert!(plan.clean_descriptor_before);
    }

    #[test]
    fn encodes_write_direction_and_dma_route_without_clobbering_bits() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        assert_eq!(plan.descriptor.command,
                   COMMAND_INTERRUPT | COMMAND_MEMORY_TO_DEVICE);
        assert!(!plan.invalidate_buffer_after);
        assert_eq!(route_sdio_to_dma1(0xa5a5_7fff),
                   (0xa5a5_7fff & !DMA_ROUTE_MASK) | DMA1_ROUTE);
    }

    #[test]
    fn rejects_unsafe_or_unrepresentable_transfers() {
        assert_eq!(build_transfer(0x2000, 0, 0, 0, 1, Direction::DeviceToMemory),
                   Err(PlanError::EmptyTransfer));
        assert_eq!(build_transfer(0x2000, 0, 0, 3, 1, Direction::DeviceToMemory),
                   Err(PlanError::UnalignedLength));
        assert_eq!(build_transfer(0x2004, 0, 0, 4, 1, Direction::DeviceToMemory),
                   Err(PlanError::UnalignedDescriptor));
        assert_eq!(build_transfer(0x2000, 0, 0, 4, 0, Direction::DeviceToMemory),
                   Err(PlanError::InvalidBurst));
        assert_eq!(build_transfer(0x2000, 0, u32::MAX as u64 + 1, 4, 1,
                                  Direction::DeviceToMemory),
                   Err(PlanError::AddressOutOfRange));
    }

    #[test]
    fn leases_one_channel_per_provider_without_reuse_while_busy() {
        use crate::topology::{DmaControllerDescription, InterruptSpec, NamedResource,
                              ResourceSpecifier};
        use api_v0::MmioRegion;
        let controller = DmaControllerDescription {
            phandle : 7,
            mmio : MmioRegion { base : 0x1fe0_0c10, size : 8 },
            interrupt : InterruptSpec { parent_phandle : 1,
                                        cells : [13, 4, 0, 0],
                                        cell_count : 2 },
            clock : NamedResource { name : None,
                                    specifier : ResourceSpecifier { provider_phandle : 2,
                                                                    args : vec![0] } },
            channel_cells : 1,
        };
        let mut leases = ChannelLeases::from_topology(&[controller]);
        let lease = leases.claim(7, 0).unwrap();
        assert_eq!(leases.claim(7, 0), Err(LeaseError::Busy));
        assert_eq!(leases.claim(7, 1), Err(LeaseError::InvalidChannel));
        assert_eq!(leases.claim(8, 0), Err(LeaseError::UnknownProvider));
        leases.release(lease).unwrap();
        assert!(leases.claim(7, 0).is_ok());
    }

    #[derive(Default)]
    struct MockOrderIo {
        value : u64,
        writes : Vec<u64>,
    }

    #[derive(Default)]
    struct MockMmcRegisters {
        writes : Vec<(usize, u32)>,
        fail_write : Option<usize>,
    }

    impl crate::mmc::RegisterIo for MockMmcRegisters {
        fn read32(&mut self, _offset : usize) -> Result<u32, dw_mmc::mmc::MmcError> {
            panic!("read publisher must remain write-only")
        }

        fn write32(&mut self, offset : usize, value : u32)
                   -> Result<(), dw_mmc::mmc::MmcError> {
            self.writes.push((offset, value));
            if self.fail_write == Some(self.writes.len()) {
                Err(dw_mmc::mmc::MmcError::RegisterOutOfRange)
            } else {
                Ok(())
            }
        }
    }

    struct MockMmcCompletionRegisters {
        interrupts : u32,
        response : u32,
        fail_first_read : bool,
        operations : Vec<(bool, usize, u32)>,
    }

    #[derive(Default)]
    struct ModelLioIo;

    impl crate::liointc::RegisterIo for ModelLioIo {
        fn read32(&self, _address : usize) -> u32 { 0 }
        fn write32(&mut self, _address : usize, _value : u32) {}
        fn write8(&mut self, _address : usize, _value : u8) {}
    }

    fn read_irq_runtime(
        mmc_irq : GlobalIrq,
        dma_irq : GlobalIrq,
        mmc_interrupts : u32)
        -> crate::irq_runtime::BoardIrqRuntime<
            ModelLioIo,
            crate::board_irq_owner::BoardIrqOwner<MockMmcCompletionRegisters>> {
        let bank0 = crate::liointc::LioIntc::new(ModelLioIo, 0, 0x1000, &[0x2000]).unwrap();
        let bank1 = crate::liointc::LioIntc::new(ModelLioIo, 1, 0x1040, &[0x2040]).unwrap();
        let mut runtime = crate::irq_runtime::BoardIrqRuntime::new(
            [Some(bank0), Some(bank1)], [None; 8]).unwrap();
        runtime.register(
            mmc_irq,
            crate::board_irq_owner::BoardIrqOwner::MmcCommand(
                crate::board_irq_owner::MmcCommandOwner::new(
                    mmc_irq,
                    MockMmcCompletionRegisters {
                        interrupts : mmc_interrupts,
                        response : 0,
                        fail_first_read : false,
                        operations : Vec::new(),
                    }))).unwrap_or_else(|_| panic!("register MMC owner failed"));
        runtime.register(
            dma_irq,
            crate::board_irq_owner::BoardIrqOwner::ApbDmaDeferred(
                crate::board_irq_owner::DeferredApbDmaOwner::new(dma_irq)))
            .unwrap_or_else(|_| panic!("register DMA owner failed"));
        runtime
    }

    impl crate::mmc::RegisterIo for MockMmcCompletionRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, dw_mmc::mmc::MmcError> {
            self.operations.push((true, offset, 0));
            if self.fail_first_read && self.operations.len() == 1 {
                return Err(dw_mmc::mmc::MmcError::RegisterOutOfRange);
            }
            match offset {
                0x3C => Ok(self.interrupts),
                0x14 => Ok(self.response),
                _ => Err(dw_mmc::mmc::MmcError::RegisterOutOfRange),
            }
        }

        fn write32(&mut self, offset : usize, value : u32)
                   -> Result<(), dw_mmc::mmc::MmcError> {
            self.operations.push((false, offset, value));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FaultOrderIo {
        value : u64,
        writes : Vec<u64>,
        write_calls : usize,
        failures : Vec<(usize, WriteEffect)>,
        confirmations : Vec<Result<bool, ExecutorError>>,
        confirmation_calls : usize,
    }

    #[derive(Default)]
    struct MockStatusReader {
        status : u32,
        calls : usize,
        fail : bool,
    }

    impl DescriptorStatusReader for MockStatusReader {
        fn read_status(&mut self, _descriptor : DmaRegion)
                       -> Result<u32, DescriptorStatusError> {
            self.calls += 1;
            if self.fail { Err(DescriptorStatusError::Read) } else { Ok(self.status) }
        }
    }

    struct FixtureStatusDecoder;

    impl DescriptorStatusDecoder for FixtureStatusDecoder {
        fn decode(&self, status : u32)
                  -> Result<DescriptorCompletion, DescriptorStatusError> {
            match status {
                0x100 => Ok(DescriptorCompletion::Complete),
                value if value & 0x8000_0000 != 0 => {
                    Ok(DescriptorCompletion::HardwareError(value))
                },
                value => Err(DescriptorStatusError::Unknown(value)),
            }
        }
    }

    #[derive(Default)]
    struct MockCache {
        device_syncs : usize,
        cpu_syncs : usize,
        fail_device : bool,
        fail_cpu_syncs : usize,
    }
    impl DmaCoherency for MockCache {
        fn sync_for_device(&mut self, _region : DmaRegion, _direction : DmaDirection)
                           -> DriverResult<()> {
            if self.fail_device { return Err(DriverError::IoError); }
            self.device_syncs += 1;
            Ok(())
        }
        fn sync_for_cpu(&mut self, _region : DmaRegion, _direction : DmaDirection)
                        -> DriverResult<()> {
            if self.fail_cpu_syncs > 0 {
                self.fail_cpu_syncs -= 1;
                return Err(DriverError::IoError);
            }
            self.cpu_syncs += 1;
            Ok(())
        }
    }

    fn mappings(plan : TransferPlan) -> (DmaMapping<MockCache>, DmaMapping<MockCache>) {
        let descriptor = DmaRegion::new(0x4000,
                                        plan.descriptor_physical_address,
                                        64,
                                        32,
                                        64).unwrap();
        let payload = DmaRegion::new(0x8000,
                                     plan.memory_physical_address,
                                     plan.byte_length,
                                     32,
                                     64).unwrap();
        (DmaMapping::new(descriptor, DmaDirection::Bidirectional, MockCache::default()),
         DmaMapping::new(payload, dma_direction(plan.direction), MockCache::default()))
    }
    impl OrderIo for MockOrderIo {
        fn read64(&mut self) -> Result<u64, ExecutorError> { Ok(self.value) }
        fn write64(&mut self, value : u64) -> Result<(), OrderWriteFailure> {
            self.value = value;
            self.writes.push(value);
            Ok(())
        }
        fn confirm_stopped(&mut self) -> Result<bool, ExecutorError> { Ok(true) }
    }

    impl OrderIo for FaultOrderIo {
        fn read64(&mut self) -> Result<u64, ExecutorError> { Ok(self.value) }
        fn write64(&mut self, value : u64) -> Result<(), OrderWriteFailure> {
            self.write_calls += 1;
            self.writes.push(value);
            if let Some(position) = self.failures.iter()
                                                 .position(|(call, _)| *call == self.write_calls)
            {
                let (_, effect) = self.failures.remove(position);
                if effect == WriteEffect::MayHaveWritten { self.value = value; }
                return Err(OrderWriteFailure { error : ExecutorError::Register, effect });
            }
            self.value = value;
            Ok(())
        }
        fn confirm_stopped(&mut self) -> Result<bool, ExecutorError> {
            self.confirmation_calls += 1;
            if self.confirmations.is_empty() { Ok(true) } else { self.confirmations.remove(0) }
        }
    }

    #[test]
    fn executor_requires_prepared_token_and_tracks_completion() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let prepared = prepare_transfer(plan, &mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        assert!(executor.start(prepared).is_ok());
        let completion = executor.complete_irq(acknowledged_dma_irq()).unwrap();
        assert!(completion.requires_buffer_invalidate());
        finish_transfer(completion, &mut descriptor, &mut payload).unwrap();
        assert!(descriptor.cpu_region().is_ok());

        assert!(payload.cpu_region().is_ok());
        let (error, acknowledged) = executor.complete_irq(acknowledged_dma_irq()).unwrap_err();
        assert_eq!(error, ExecutorError::Idle);
        assert_eq!(acknowledged.irq(), dma_irq());
        let io = executor.into_inner();
        assert_eq!(io.writes, vec![0, plan.start_order]);
    }

    #[test]
    fn executor_rejects_overlap_and_encodes_stop() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        assert!(executor.start(prepare_transfer(plan, &mut descriptor, &mut payload).unwrap())
                        .is_ok());
        let (mut descriptor2, mut payload2) = mappings(plan);
        let failure = executor.start(prepare_transfer(plan,
                                                      &mut descriptor2,
                                                      &mut payload2).unwrap())
                              .unwrap_err();
        assert_eq!(failure.error, ExecutorError::Busy);
        cancel_prepared(&failure.prepared, &mut descriptor2, &mut payload2).unwrap();
        let completion = executor.stop().unwrap();
        finish_transfer(completion, &mut descriptor, &mut payload).unwrap();
        assert_eq!(executor.into_inner().writes.last().copied(),
                   Some((plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)));
    }

    #[test]
    fn mapping_mismatch_and_payload_sync_failure_are_recoverable() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let wrong = DmaRegion::new(0xa000, 0x4000, 512, 32, 64).unwrap();
        let mut wrong_payload = DmaMapping::new(wrong, DmaDirection::FromDevice,
                                               MockCache::default());
        assert!(matches!(prepare_transfer(plan, &mut descriptor, &mut wrong_payload),
                         Err(DriverError::InvalidParam)));
        assert!(descriptor.cpu_region().is_ok());

        let mut malformed = plan;
        malformed.descriptor.memory_address_low ^= 1;
        assert!(matches!(prepare_transfer(malformed, &mut descriptor, &mut wrong_payload),
                         Err(DriverError::InvalidParam)));
        assert!(descriptor.is_cpu_owned());
        assert!(wrong_payload.is_cpu_owned());

        let payload_region = payload.cpu_region().unwrap();
        payload = DmaMapping::new(payload_region,
                                  DmaDirection::FromDevice,
                                  MockCache { fail_device : true, ..MockCache::default() });
        assert!(matches!(prepare_transfer(plan, &mut descriptor, &mut payload),
                         Err(DriverError::IoError)));
        assert!(descriptor.cpu_region().is_ok());
        assert!(payload.cpu_region().is_ok());
    }

    #[test]
    fn typestate_session_covers_irq_completion() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());

        let running = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                          .start(&mut executor)
                                                                          .unwrap();
        let completion = running.complete_irq(acknowledged_dma_irq()).unwrap();
        // SAFETY: the mock IRQ deterministically marks this transfer stopped.
        unsafe { completion.reclaim_unverified() }.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn quiesced_handoff_returns_only_cpu_owned_mappings() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let handoff = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                            .start(&mut executor)
                                                                            .unwrap()
                                                                            .stop()
                                                                            .unwrap()
                                                                            .into_handoff();
        let cpu_owned = match handoff.finish() {
            Ok(cpu_owned) => cpu_owned,
            Err(_) => panic!("quiesced handoff did not finish"),
        };
        let (descriptor_mapping, payload_mapping) = cpu_owned.into_mappings();
        assert!(descriptor_mapping.is_cpu_owned());
        assert!(payload_mapping.is_cpu_owned());
        assert!(descriptor_mapping.cpu_region().is_ok());
        assert!(payload_mapping.cpu_region().is_ok());
    }

    #[test]
    fn mmc_adapter_keeps_same_handoff_across_sync_retry() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let descriptor_region = DmaRegion::new(0x4000, 0x2000, 64, 32, 64).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x3000, 512, 32, 64).unwrap();
        let mut descriptor = DmaMapping::new(
            descriptor_region,
            DmaDirection::Bidirectional,
            MockCache { fail_cpu_syncs : 1, ..MockCache::default() });
        let mut payload = DmaMapping::new(payload_region,
                                          DmaDirection::FromDevice,
                                          MockCache::default());
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let handoff = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                            .start(&mut executor)
                                                                            .unwrap()
                                                                            .stop()
                                                                            .unwrap()
                                                                            .into_handoff();
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read_plan = *recovery.plan();
        let mut invalid_plan = read_plan;
        invalid_plan.request.byte_length += 4;
        let failure = match crate::mmc::ReadDmaQuiescedEvidence::bind_apbdma_handoff(
            &invalid_plan, handoff)
        {
            Ok(_) => panic!("internally inconsistent MMC read plan accepted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, crate::mmc::ReadDmaIdentityError::ReadPlanInvalid);
        let mut wrong_length = read_plan;
        wrong_length.request = crate::mmc::ReadBlockRequest::new(
            0, 2, 512, crate::mmc::ReadAddressing::Block).unwrap();
        wrong_length.setup_writes[0].value = (read_plan.setup_writes[0].value & !0xFFF) | 2;
        let failure = match crate::mmc::ReadDmaQuiescedEvidence::bind_apbdma_handoff(
            &wrong_length, failure.handoff)
        {
            Ok(_) => panic!("valid MMC plan with wrong DMA byte length accepted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, crate::mmc::ReadDmaIdentityError::ByteLength);
        let mut wrong_data = read_plan;
        wrong_data.data_register_address += 4;
        let failure = match crate::mmc::ReadDmaQuiescedEvidence::bind_apbdma_handoff(
            &wrong_data, failure.handoff)
        {
            Ok(_) => panic!("wrong MMC DATA address accepted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, crate::mmc::ReadDmaIdentityError::DataRegisterAddress);
        let (evidence, mut synchronizer) =
            match crate::mmc::ReadDmaQuiescedEvidence::bind_apbdma_handoff(
                &read_plan, failure.handoff)
            {
                Ok(parts) => parts,
                Err(_) => panic!("matching APBDMA handoff rejected"),
            };
        let clean = crate::mmc::CommandPostSnapshot { argument : 0,
                                                       control : 0,
                                                       command_status : 0,
                                                       data_status : 0,
                                                       interrupts : 0 };
        let recovery = match recovery.record_mmc_quiesced(clean, clean) {
            Ok(recovery) => recovery,
            Err(_) => panic!("clean MMC recovery evidence rejected"),
        };
        let recovery = match recovery.record_dma_quiesced(evidence) {
            Ok(recovery) => recovery,
            Err(_) => panic!("APBDMA handoff evidence rejected"),
        };
        let failure = match recovery.sync_for_cpu(&mut synchronizer) {
            Ok(_) => panic!("fault-injected APBDMA sync succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error,
                   crate::mmc::ReadCombinedRecoveryError::SyncForCpu(DriverError::IoError));
        let recovered = match failure.recovery.sync_for_cpu(&mut synchronizer) {
            Ok(recovered) => recovered,
            Err(_) => panic!("APBDMA sync retry failed"),
        };
        let mut marker = recovered.into_buffer();
        assert_eq!(marker, 0);
        assert_eq!(crate::mmc::ReadRecoverySync::sync_for_cpu(&mut synchronizer,
                                                              &mut marker),
                   Err(DriverError::InvalidParam));
        let cpu_owned = synchronizer.into_cpu_owned().expect("missing CPU-owned handoff");
        let (descriptor_mapping, payload_mapping) = cpu_owned.into_mappings();
        assert!(descriptor_mapping.is_cpu_owned());
        assert!(payload_mapping.is_cpu_owned());
    }

    #[test]
    fn mmc_adapter_rejects_memory_to_device_handoff_without_losing_it() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let handoff = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                            .start(&mut executor)
                                                                            .unwrap()
                                                                            .stop()
                                                                            .unwrap()
                                                                            .into_handoff();
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let failure = match crate::mmc::ReadDmaQuiescedEvidence::bind_apbdma_handoff(
            recovery.plan(), handoff)
        {
            Ok(_) => panic!("memory-to-device handoff accepted as MMC read"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, crate::mmc::ReadDmaIdentityError::TransferDirection);
        let cpu_owned = failure.handoff.finish().unwrap();
        let (descriptor, payload) = cpu_owned.into_mappings();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn mmc_read_start_typestate_orders_dma_before_command_publish() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut wrong_read = read;
        wrong_read.data_register_address += 4;
        assert_eq!(crate::mmc::ReadDmaBinding::bind(&wrong_read,
                                                    transfer,
                                                    &descriptor,
                                                    &payload),
                   Err(crate::mmc::ReadDmaIdentityError::DataRegisterAddress));
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        let binding = crate::mmc::ReadDmaBinding::bind(&read,
                                                       transfer,
                                                       &descriptor,
                                                       &payload).unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        let prepared = binding.prepare(&mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let running = armed_read_irqs(101).bind_prepared_dma(prepared)
                                             .start(&mut executor).unwrap();
        assert_eq!(running.transaction().raw(), 101);
        assert_eq!(running.plan(), &read);
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        let published = running.publish(&mut publisher).unwrap();
        assert_eq!(published.plan(), &read);
        assert_eq!(publisher.into_inner().writes.iter()
                                           .map(|(offset, _)| *offset)
                                           .collect::<Vec<_>>(),
                   [0x2C, 0x28, 0x24, 0x3C, 0x08, 0x0C]);
        let armed = published.stop().unwrap().finish().unwrap();
        assert_eq!(armed.transaction().raw(), 101);
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().writes,
                   vec![0,
                        transfer.start_order,
                        (transfer.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | ORDER_STOP]);
    }

    #[test]
    fn mmc_read_start_failures_preserve_precise_dma_ownership() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();

        let (mut descriptor, mut payload) = mappings(transfer);
        let binding = crate::mmc::ReadDmaBinding::bind(&read,
                                                       transfer,
                                                       &descriptor,
                                                       &payload).unwrap();
        let payload_region = payload.cpu_region().unwrap();
        payload = DmaMapping::new(payload_region,
                                  DmaDirection::FromDevice,
                                  MockCache { fail_device : true, ..MockCache::default() });
        assert!(matches!(binding.prepare(&mut descriptor, &mut payload),
                         Err(DriverError::IoError)));
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());

        let (mut descriptor, mut payload) = mappings(transfer);
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let registers = FaultOrderIo { failures : vec![(1, WriteEffect::Untouched)],
                                       ..FaultOrderIo::default() };
        let mut executor = Executor::new(registers, dma_irq());
        let failure = match armed_read_irqs(102).bind_prepared_dma(prepared)
                                                .start(&mut executor) {
            Ok(_) => panic!("untouched start fault accepted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.transaction().raw(), 102);
        let armed = match failure {
            crate::board_irq_owner::IrqArmedReadDmaStartFailure::Prepared {
                read : failed_read, session, ..
            } => {
                assert_eq!(failed_read, read);
                session.cancel().unwrap()
            },
            crate::board_irq_owner::IrqArmedReadDmaStartFailure::Recovery { .. } =>
                panic!("untouched write entered recovery"),
        };
        assert_eq!(armed.transaction().raw(), 102);
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());

        let (mut descriptor, mut payload) = mappings(transfer);
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let registers = FaultOrderIo { failures : vec![(2, WriteEffect::MayHaveWritten)],
                                       ..FaultOrderIo::default() };
        let mut executor = Executor::new(registers, dma_irq());
        let failure = match armed_read_irqs(103).bind_prepared_dma(prepared)
                                                .start(&mut executor) {
            Ok(_) => panic!("uncertain start fault accepted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.transaction().raw(), 103);
        let armed = match failure {
            crate::board_irq_owner::IrqArmedReadDmaStartFailure::Recovery { session, .. } =>
                session.stop().unwrap().finish().unwrap(),
            crate::board_irq_owner::IrqArmedReadDmaStartFailure::Prepared { .. } =>
                panic!("uncertain write remained cancellable"),
        };
        assert_eq!(armed.transaction().raw(), 103);
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn mmc_publish_failure_keeps_running_dma_for_explicit_stop() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let (mut descriptor, mut payload) = mappings(transfer);
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let running = armed_read_irqs(104).bind_prepared_dma(prepared)
                                             .start(&mut executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters { fail_write : Some(5), ..MockMmcRegisters::default() },
            crate::mmc::ReadDataPublishPermit::fixture());
        let failure = match running.publish(&mut publisher) {
            Ok(_) => panic!("fault-injected MMC publish succeeded"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error,
                   crate::mmc::ReadDataPublishFailure {
                       error : crate::mmc::ReadDataPublishError::Io(
                           dw_mmc::mmc::MmcError::RegisterOutOfRange),
                       stage : Some(crate::mmc::ReadDataPublishStage::CommandArgument),
                       writes_completed : 4,
                   });
        assert_eq!(publisher.into_inner().writes.len(), 5);
        let armed = failure.session.stop().unwrap().finish().unwrap();
        assert_eq!(armed.transaction().raw(), 104);
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[derive(Clone, Copy)]
    enum ReadFact {
        Command,
        Data,
        Dma,
    }

    fn record_read_fact<B>(tracker : crate::mmc::ReadCompletionTracker<B>,
                           fact : ReadFact)
                           -> crate::mmc::ReadCompletionProgress<B> {
        match fact {
            ReadFact::Command => tracker.command_validated(),
            ReadFact::Data => tracker.controller_interrupt(1),
            ReadFact::Dma => tracker.dma_completed(),
        }
    }

    fn published_read_tracker<'a, 'e>(
        transfer : TransferPlan,
        descriptor : &'a mut DmaMapping<MockCache>,
        payload : &'a mut DmaMapping<MockCache>,
        executor : &'e mut Executor<MockOrderIo>)
        -> crate::mmc::ReadCompletionTracker<
            crate::mmc::PublishedReadDmaSession<'a, 'e, MockOrderIo, MockCache, MockCache>> {
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let running = crate::mmc::ReadDmaBinding::bind(&read,
                                                       transfer,
                                                       descriptor,
                                                       payload).unwrap()
                                                   .prepare(descriptor, payload).unwrap()
                                                   .start(executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        running.publish(&mut publisher).unwrap()
               .into_completion_tracker()
    }

    fn paired_acknowledged_session<'a, 'e>(
        transfer : TransferPlan,
        descriptor : &'a mut DmaMapping<MockCache>,
        payload : &'a mut DmaMapping<MockCache>,
        executor : &'e mut Executor<MockOrderIo>,
        transaction_raw : u64,
        mmc_interrupts : u32)
        -> (crate::board_irq_owner::PairedAcknowledgedReadDmaSession<
                'a, MockCache, MockCache>,
            crate::read_coordinator::ReadCoordinatorSlot) {
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        descriptor,
                                                        payload).unwrap()
                                                    .prepare(descriptor, payload).unwrap();
        let transaction = crate::board_irq_owner::ReadTransactionId::new(
            transaction_raw).unwrap();
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut runtime = read_irq_runtime(mmc_irq, dma_irq(), mmc_interrupts);
        let armed = crate::board_irq_owner::reserve_read_irq_owners(
            &mut runtime, mmc_irq, dma_irq(), transaction)
            .unwrap_or_else(|_| panic!("reserve read owners failed"))
            .commit();
        let running = armed.bind_prepared_dma(prepared).start(executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        let published = running.publish(&mut publisher).unwrap();
        let coordinator = crate::read_coordinator::ReadCoordinatorSlot::new();
        coordinator.reserve(transaction).unwrap().commit();
        coordinator.mark_published(transaction, 1).unwrap();
        coordinator.record_recheck(
            crate::board_irq_owner::BoundedMmcReadRecheck::new(transaction, 1).unwrap())
            .unwrap_or_else(|_| panic!("bounded recheck token rejected"));
        assert_eq!(runtime.owner_mut(dma_irq()).unwrap()
                          .handle(acknowledged_dma_irq()),
                   crate::irq_domain::IrqDisposition::KeepMasked);
        assert_eq!(coordinator.service_recheck(transaction).unwrap()
                              .step(&mut runtime, mmc_irq),
                   Ok(crate::read_coordinator::ReadCoordinatorStepProgress::Terminal {
                       transaction, polls_completed : 1,
                   }));
        let paired = published.claim_pending_pair(
            coordinator.service_terminal(transaction).unwrap(),
            &mut runtime, mmc_irq, dma_irq())
            .unwrap_or_else(|_| panic!("matching pair rejected"))
            .acknowledge_dma_irq()
            .unwrap_or_else(|_| panic!("paired DMA IRQ rejected"));
        (paired, coordinator)
    }

    #[test]
    fn published_read_tracker_keeps_dma_running_across_all_success_orders() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let orders = [[ReadFact::Command, ReadFact::Data, ReadFact::Dma],
                      [ReadFact::Command, ReadFact::Dma, ReadFact::Data],
                      [ReadFact::Data, ReadFact::Command, ReadFact::Dma],
                      [ReadFact::Data, ReadFact::Dma, ReadFact::Command],
                      [ReadFact::Dma, ReadFact::Command, ReadFact::Data],
                      [ReadFact::Dma, ReadFact::Data, ReadFact::Command]];
        for order in orders {
            let (mut descriptor, mut payload) = mappings(transfer);
            let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
            let mut tracker = Some(published_read_tracker(transfer,
                                                          &mut descriptor,
                                                          &mut payload,
                                                          &mut executor));
            for (index, fact) in order.into_iter().enumerate() {
                let progress = record_read_fact(tracker.take().unwrap(), fact);
                if index < 2 {
                    tracker = Some(match progress {
                        crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
                        _ => panic!("published read completed before all facts"),
                    });
                } else {
                    let completed = match progress {
                        crate::mmc::ReadCompletionProgress::Completed(completed) => completed,
                        _ => panic!("published read did not complete after all facts"),
                    };
                    assert_eq!(completed.evidence,
                               crate::mmc::ReadCompletionEvidence {
                                   command_response_validated : true,
                                   data_finished : true,
                                   dma_finished : true,
                               });
                    completed.into_published_session()
                             .stop().unwrap()
                             .finish().unwrap();
                }
            }
            assert!(descriptor.is_cpu_owned());
            assert!(payload.is_cpu_owned());
        }
    }

    #[test]
    fn published_read_dma_irq_and_status_complete_the_tracker() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor);
        let tracker = match tracker.command_validated() {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("command fact prematurely completed the read"),
        };
        let tracker = match tracker.controller_interrupt(1) {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("data fact prematurely completed the read"),
        };
        let tracker = tracker.acknowledge_dma_irq(acknowledged_dma_irq()).unwrap();
        let mut reader = MockStatusReader { status : 0x100, ..MockStatusReader::default() };
        let completed = match tracker.inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                     .unwrap() {
            crate::mmc::ReadCompletionProgress::Completed(completed) => completed,
            _ => panic!("verified DMA completion did not complete the read"),
        };
        assert_eq!(completed.evidence,
                   crate::mmc::ReadCompletionEvidence {
                       command_response_validated : true,
                       data_finished : true,
                       dma_finished : true,
                   });
        completed.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn published_read_retries_wrong_irq_and_status_observation() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let descriptor_region = DmaRegion::new(0x4000, 0x2000, 64, 32, 64).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x3000, 512, 32, 64).unwrap();
        let mut descriptor = DmaMapping::new(
            descriptor_region,
            DmaDirection::Bidirectional,
            MockCache { fail_cpu_syncs : 1, ..MockCache::default() });
        let mut payload = DmaMapping::new(payload_region,
                                          DmaDirection::FromDevice,
                                          MockCache::default());
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor);
        let wrong_irq = GlobalIrq::from_bank_local(0, 13).unwrap();
        let wrong = AcknowledgedIrq::after_mask_ack(wrong_irq);
        let failure = match tracker.acknowledge_dma_irq(wrong) {
            Err(failure) => failure,
            Ok(_) => panic!("wrong IRQ advanced the read tracker"),
        };
        assert_eq!(failure.error, ExecutorError::UnexpectedIrq);
        assert_eq!(failure.acknowledged.irq(), wrong_irq);
        let tracker = failure.tracker
                             .acknowledge_dma_irq(acknowledged_dma_irq()).unwrap();

        let mut reader = MockStatusReader { status : 0x100,
                                            ..MockStatusReader::default() };
        let failure = match tracker.inspect_dma_status(&mut reader, &FixtureStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("failed descriptor cache sync advanced the tracker"),
        };
        assert_eq!(failure.error, DescriptorStatusError::Cache(DriverError::IoError));
        assert_eq!(reader.calls, 0);
        reader.fail = true;
        let failure = match failure.tracker
                                   .inspect_dma_status(&mut reader, &FixtureStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("failed descriptor read advanced the tracker"),
        };
        assert_eq!(failure.error, DescriptorStatusError::Read);
        reader.fail = false;
        reader.status = 0x42;
        let failure = match failure.tracker
                                   .inspect_dma_status(&mut reader, &FixtureStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("unknown descriptor status advanced the tracker"),
        };
        assert_eq!(failure.error, DescriptorStatusError::Unknown(0x42));
        reader.status = 0x100;
        let tracker = match failure.tracker
                                   .inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                   .unwrap() {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("DMA fact completed the read before command/data"),
        };
        let tracker = match tracker.command_validated() {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("command fact completed the read before data"),
        };
        let completed = match tracker.controller_interrupt(1) {
            crate::mmc::ReadCompletionProgress::Completed(completed) => completed,
            _ => panic!("all three facts did not complete the read"),
        };
        completed.into_quiesced_session().finish().unwrap();
        assert_eq!(reader.calls, 3);
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn published_read_descriptor_hardware_error_enters_quiesced_recovery() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor)
                          .acknowledge_dma_irq(acknowledged_dma_irq()).unwrap();
        let mut reader = MockStatusReader { status : 0x8000_0042,
                                            ..MockStatusReader::default() };
        let recovery = match tracker.inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                    .unwrap() {
            crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
            _ => panic!("descriptor hardware error did not enter recovery"),
        };
        assert_eq!(recovery.failure,
                   crate::mmc::ReadCompletionFailure::Dma(
                       crate::mmc::ReadDmaFailure::Hardware(0x8000_0042)));
        recovery.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn board_owner_hands_one_irq_to_fail_closed_read_status_path() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor);
        let mut owner = crate::board_irq_owner::DeferredApbDmaOwner::new(dma_irq());
        let transaction = crate::board_irq_owner::ReadTransactionId::new(1).unwrap();
        owner.arm_read(transaction).unwrap();
        assert_eq!(owner.handle(acknowledged_dma_irq()),
                   crate::irq_domain::IrqDisposition::KeepMasked);
        let receipt = owner.take_read_receipt().unwrap();
        assert_eq!(receipt.transaction, transaction);
        assert_eq!(owner.take_read_receipt(), None);
        let tracker = tracker.acknowledge_dma_irq(receipt.acknowledged).unwrap();
        let mut reader = MockStatusReader { status : 0x100,
                                            ..MockStatusReader::default() };
        let failure = match tracker.inspect_dma_status(&mut reader, &UnverifiedStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("production decoder invented APBDMA completion"),
        };
        assert_eq!(failure.error, DescriptorStatusError::StatusUnverified);
        assert_eq!(reader.calls, 1);

        // Fixture-only decoding releases the model resources after proving the
        // production path retained the same acknowledged session for retry.
        let tracker = match failure.tracker
                                   .inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                   .unwrap() {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("DMA-only evidence completed the read"),
        };
        let recovery = match tracker.command_failed(crate::mmc::ReadCommandFailure::Io) {
            crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
            _ => panic!("fixture cleanup did not retain quiesced DMA"),
        };
        recovery.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn paired_mmc_and_dma_receipts_complete_only_their_read_generation() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let transaction = crate::board_irq_owner::ReadTransactionId::new(23).unwrap();
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut runtime = read_irq_runtime(mmc_irq, dma_irq(), (1 << 6) | 1);
        let armed = crate::board_irq_owner::reserve_read_irq_owners(
            &mut runtime, mmc_irq, dma_irq(), transaction)
            .unwrap_or_else(|_| panic!("reserve read owners failed"))
            .commit();
        let running = armed.bind_prepared_dma(prepared).start(&mut executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        let published = running.publish(&mut publisher).unwrap();
        let coordinator = crate::read_coordinator::ReadCoordinatorSlot::new();
        coordinator.reserve(transaction).unwrap().commit();
        coordinator.mark_published(transaction, 1).unwrap();
        coordinator.record_recheck(
            crate::board_irq_owner::BoundedMmcReadRecheck::new(transaction, 1).unwrap())
            .unwrap_or_else(|_| panic!("bounded recheck token rejected"));

        let dma_owner = runtime.owner_mut(dma_irq()).unwrap();
        assert_eq!(dma_owner.handle(acknowledged_dma_irq()),
                   crate::irq_domain::IrqDisposition::KeepMasked);
        assert_eq!(coordinator.service_recheck(transaction).unwrap()
                              .step(&mut runtime, mmc_irq),
                   Ok(crate::read_coordinator::ReadCoordinatorStepProgress::Terminal {
                       transaction, polls_completed : 1,
                   }));
        let failure = match published.claim_pending_pair(
            coordinator.service_terminal(transaction).unwrap(),
            &mut runtime, dma_irq(), mmc_irq)
        {
            Err(failure) => failure,
            Ok(_) => panic!("reversed owner slots claimed completion"),
        };
        assert_eq!(failure.error,
                   crate::read_coordinator::ReadTerminalClaimError::Pair(
                       crate::board_irq_owner::ReadPendingPairError::MmcOwnerVariant));
        assert_eq!(coordinator.snapshot().unwrap().phase,
                   crate::read_coordinator::ReadCoordinatorPhase::Terminal);
        let published = failure.session;
        let paired = published.claim_pending_pair(
            coordinator.service_terminal(transaction).unwrap(),
            &mut runtime, mmc_irq, dma_irq())
            .unwrap_or_else(|_| panic!("matching pair rejected"));
        assert_eq!(coordinator.snapshot().unwrap().phase,
                   crate::read_coordinator::ReadCoordinatorPhase::CompletionClaimed);
        match coordinator.service_terminal(transaction) {
            Err(error) => assert_eq!(error,
                crate::read_coordinator::ReadCoordinatorError::WrongPhase {
                    expected : crate::read_coordinator::ReadCoordinatorPhase::Terminal,
                    actual : crate::read_coordinator::ReadCoordinatorPhase::CompletionClaimed,
                }),
            Ok(_) => panic!("completion pair was claimed twice"),
        }
        let paired = paired.acknowledge_dma_irq()
                           .unwrap_or_else(|_| panic!("paired DMA IRQ rejected"));
        assert_eq!(paired.mmc.transaction, transaction);
        let mut reader = MockStatusReader { status : 0x100,
                                            ..MockStatusReader::default() };
        let failure = match paired.inspect_dma_status(&mut reader, &UnverifiedStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("production decoder accepted paired status"),
        };
        assert_eq!(failure.error, DescriptorStatusError::StatusUnverified);
        assert_eq!(failure.session.mmc.transaction, transaction);
        let paired = match failure.session
                                  .inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                  .unwrap_or_else(|_| panic!("paired status retry failed")) {
            crate::board_irq_owner::PairedDmaStatusProgress::Pending(paired) => paired,
            _ => panic!("DMA receipt completed read before MMC receipt"),
        };
        let completed = match paired.apply_mmc_receipt() {
            crate::mmc::ReadCompletionProgress::Completed(completed) => completed,
            _ => panic!("paired receipts did not complete their read"),
        };
        assert_eq!(completed.evidence,
                   crate::mmc::ReadCompletionEvidence {
                       command_response_validated : true,
                       data_finished : true,
                       dma_finished : true,
                   });
        completed.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        coordinator.release(transaction).unwrap();
        assert_eq!(coordinator.state(),
                   crate::diagnostic_slot::DiagnosticSlotState::Empty);
    }

    #[test]
    fn paired_status_and_mmc_errors_retain_quiesced_recovery() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let (paired, coordinator) = paired_acknowledged_session(transfer,
                                                                &mut descriptor,
                                                                &mut payload,
                                                                &mut executor,
                                                                25,
                                                                (1 << 6) | 1);
        let mut reader = MockStatusReader { status : 0x8000_0042,
                                            ..MockStatusReader::default() };
        let (mmc, recovery) = match paired.inspect_dma_status(&mut reader,
                                                              &FixtureStatusDecoder)
                                           .unwrap_or_else(|_| panic!("status inspect failed")) {
            crate::board_irq_owner::PairedDmaStatusProgress::RecoveryRequired {
                mmc, recovery
            } => (mmc, recovery),
            _ => panic!("hardware status did not enter paired recovery"),
        };
        assert_eq!(mmc.transaction.raw(), 25);
        assert_eq!(recovery.failure,
                   crate::mmc::ReadCompletionFailure::Dma(
                       crate::mmc::ReadDmaFailure::Hardware(0x8000_0042)));
        let wrong = crate::board_irq_owner::ReadTransactionId::new(125).unwrap();
        match coordinator.service_claimed_completion(wrong) {
            Err(error) => assert_eq!(error,
                crate::read_coordinator::ReadCoordinatorError::WrongTransaction {
                    expected : mmc.transaction, actual : wrong,
                }),
            Ok(_) => panic!("wrong generation serviced claimed completion"),
        }
        assert_eq!(coordinator.snapshot().unwrap().phase,
                   crate::read_coordinator::ReadCoordinatorPhase::CompletionClaimed);
        let cause = coordinator.service_claimed_completion(mmc.transaction).unwrap()
                               .record_failure(recovery.failure);
        assert_eq!(cause,
                   crate::board_irq_owner::ReadRecoveryCause::CompletionFailure(
                       recovery.failure));
        assert_eq!(coordinator.snapshot().unwrap().recovery_cause, Some(cause));
        assert_eq!(coordinator.release(mmc.transaction),
                   Err(crate::read_coordinator::ReadCoordinatorError::RecoveryMustBeRecorded));
        recovery.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());

        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let (paired, coordinator) = paired_acknowledged_session(transfer,
                                                                &mut descriptor,
                                                                &mut payload,
                                                                &mut executor,
                                                                26,
                                                                1 << 7);
        let mut reader = MockStatusReader { status : 0x100,
                                            ..MockStatusReader::default() };
        let paired = match paired.inspect_dma_status(&mut reader, &FixtureStatusDecoder)
                                 .unwrap_or_else(|_| panic!("status inspect failed")) {
            crate::board_irq_owner::PairedDmaStatusProgress::Pending(paired) => paired,
            _ => panic!("DMA success did not await MMC command error snapshot"),
        };
        let recovery = match paired.apply_mmc_receipt() {
            crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
            _ => panic!("MMC timeout snapshot did not enter recovery"),
        };
        assert_eq!(recovery.failure,
                   crate::mmc::ReadCompletionFailure::Command(
                       crate::mmc::ReadCommandFailure::Timeout));
        let transaction = crate::board_irq_owner::ReadTransactionId::new(26).unwrap();
        let cause = coordinator.service_claimed_completion(transaction).unwrap()
                               .record_failure(recovery.failure);
        assert_eq!(cause,
                   crate::board_irq_owner::ReadRecoveryCause::CompletionFailure(
                       recovery.failure));
        match coordinator.service_claimed_completion(transaction) {
            Err(error) => assert_eq!(error,
                crate::read_coordinator::ReadCoordinatorError::WrongPhase {
                    expected : crate::read_coordinator::ReadCoordinatorPhase::CompletionClaimed,
                    actual : crate::read_coordinator::ReadCoordinatorPhase::RecoveryPending,
                }),
            Ok(_) => panic!("completion failure was recorded twice"),
        }
        recovery.into_quiesced_session().finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn paired_session_failure_retains_both_receipts_and_running_dma() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let runtime_dma_irq = dma_irq();
        let executor_dma_irq = GlobalIrq::from_bank_local(1, 14).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), executor_dma_irq);
        let transaction = crate::board_irq_owner::ReadTransactionId::new(24).unwrap();
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut runtime = read_irq_runtime(mmc_irq, runtime_dma_irq, (1 << 6) | 1);
        let armed = crate::board_irq_owner::reserve_read_irq_owners(
            &mut runtime, mmc_irq, runtime_dma_irq, transaction)
            .unwrap_or_else(|_| panic!("reserve read owners failed"))
            .commit();
        let running = armed.bind_prepared_dma(prepared).start(&mut executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        let published = running.publish(&mut publisher).unwrap();

        assert_eq!(runtime.owner_mut(runtime_dma_irq).unwrap()
                          .handle(acknowledged_dma_irq()),
                   crate::irq_domain::IrqDisposition::KeepMasked);
        assert_eq!(runtime.owner_mut(mmc_irq).unwrap()
                          .handle(AcknowledgedIrq::after_mask_ack(mmc_irq)),
                   crate::irq_domain::IrqDisposition::KeepMasked);
        let paired = published.take_pending_pair(&mut runtime, mmc_irq, runtime_dma_irq)
                              .unwrap_or_else(|_| panic!("matching pair rejected"));
        let failure = match paired.acknowledge_dma_irq() {
            Err(failure) => failure,
            Ok(_) => panic!("mismatched executor IRQ accepted"),
        };
        assert_eq!(failure.mmc.transaction, transaction);
        assert_eq!(failure.dma_transaction, transaction);
        assert_eq!(failure.failure.error, ExecutorError::UnexpectedIrq);
        assert_eq!(failure.failure.acknowledged.irq(), runtime_dma_irq);
        let recovery = match failure.failure.tracker
                                            .command_failed(
                                                crate::mmc::ReadCommandFailure::Io) {
            crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
            _ => panic!("running DMA was not retained for recovery"),
        };
        recovery.into_published_session()
                .stop().unwrap()
                .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn bounded_mmc_timeout_stops_dma_before_retiring_mixed_owner_state() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let recovery = crate::mmc::combined_recovery_fixture(0u8);
        let read = *recovery.plan();
        let prepared = crate::mmc::ReadDmaBinding::bind(&read,
                                                        transfer,
                                                        &descriptor,
                                                        &payload).unwrap()
                                                    .prepare(&mut descriptor, &mut payload).unwrap();
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let transaction = crate::board_irq_owner::ReadTransactionId::new(27).unwrap();
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut runtime = read_irq_runtime(mmc_irq, dma_irq(), 0);
        let armed = crate::board_irq_owner::reserve_read_irq_owners(
            &mut runtime, mmc_irq, dma_irq(), transaction)
            .unwrap_or_else(|_| panic!("reserve read owners failed"))
            .commit();
        let running = armed.bind_prepared_dma(prepared).start(&mut executor).unwrap();
        let mut publisher = crate::mmc::ReadDataCommandPublisher::new(
            MockMmcRegisters::default(),
            crate::mmc::ReadDataPublishPermit::fixture());
        let published = running.publish(&mut publisher).unwrap();
        let coordinator = crate::read_coordinator::ReadCoordinatorSlot::new();
        coordinator.reserve(transaction).unwrap().commit();
        coordinator.mark_published(transaction, 2).unwrap();
        coordinator.record_recheck(
            crate::board_irq_owner::BoundedMmcReadRecheck::new(transaction, 2).unwrap())
            .unwrap_or_else(|_| panic!("bounded recheck token rejected"));
        assert_eq!(runtime.owner_mut(dma_irq()).unwrap()
                          .handle(acknowledged_dma_irq()),
                   crate::irq_domain::IrqDisposition::KeepMasked);

        assert_eq!(coordinator.service_recheck(transaction).unwrap()
                              .step(&mut runtime, mmc_irq),
                   Ok(crate::read_coordinator::ReadCoordinatorStepProgress::Pending {
                       transaction, remaining : 1, polls_completed : 1,
                   }));
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let crate::board_irq_owner::BoardIrqOwner::MmcCommand(mmc) = mmc else {
            panic!("wrong MMC owner variant")
        };
        mmc.registers_mut().interrupts = 1 << 6;
        let cause = crate::board_irq_owner::ReadRecoveryCause::Timeout {
            polls_completed : 2,
        };
        assert_eq!(coordinator.service_recheck(transaction).unwrap()
                              .step(&mut runtime, mmc_irq),
                   Ok(crate::read_coordinator::ReadCoordinatorStepProgress::RecoveryPending {
                       transaction, cause,
                   }));
        let quiesced = published.stop().unwrap().finish_recovery().unwrap();
        assert_eq!(quiesced.transaction(), transaction);
        let failure = coordinator.service_recovery(transaction).unwrap()
            .retire_and_record(&mut runtime, dma_irq(), mmc_irq, quiesced)
            .expect_err("reversed owner slots retired");
        assert_eq!(failure.error,
                   crate::read_coordinator::ReadCoordinatorRecoveryError::Retire(
                       crate::board_irq_owner::ReadIrqRetireError::MmcOwnerVariant));
        assert_eq!(failure.cause, cause);
        let quiesced = failure.into_quiesced();
        assert_eq!(coordinator.snapshot().unwrap().phase,
                   crate::read_coordinator::ReadCoordinatorPhase::RecoveryPending);
        coordinator.service_recovery(transaction).unwrap()
                   .retire_and_record(&mut runtime, mmc_irq, dma_irq(), quiesced)
                   .unwrap_or_else(|_| panic!("timeout owners did not drain"));
        let snapshot = coordinator.snapshot().unwrap();
        assert_eq!(snapshot.phase,
                   crate::read_coordinator::ReadCoordinatorPhase::RecoveryRecorded);
        assert_eq!(snapshot.recovery_cause, Some(cause));
        assert_eq!(snapshot.partial_mmc_interrupts, Some(1 << 6));
        assert!(snapshot.has_dma_receipt);
        let report = coordinator.take_recovery(transaction).unwrap();
        assert_eq!(report.transaction, transaction);
        assert_eq!(report.cause, cause);
        assert_eq!(report.partial_mmc_interrupts, 1 << 6);
        assert!(report.drained.mmc.is_none());
        assert_eq!(report.drained.dma.unwrap().acknowledged.irq(), dma_irq());
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn published_read_errors_return_running_session_for_stop_recovery() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let data_errors = [(1 << 1, crate::mmc::ReadCompletionFailure::DataTimeout),
                           (1 << 2, crate::mmc::ReadCompletionFailure::ReceiveCrc),
                           (1 << 3, crate::mmc::ReadCompletionFailure::TransmitCrc),
                           (1 << 4, crate::mmc::ReadCompletionFailure::ProgramError),
                           (1 << 31,
                            crate::mmc::ReadCompletionFailure::UnknownInterrupt(1 << 31))];
        for (interrupts, expected) in data_errors {
            let (mut descriptor, mut payload) = mappings(transfer);
            let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
            let tracker = published_read_tracker(transfer,
                                                 &mut descriptor,
                                                 &mut payload,
                                                 &mut executor);
            let recovery = match tracker.controller_interrupt(interrupts) {
                crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
                _ => panic!("published data error did not enter recovery"),
            };
            assert_eq!(recovery.failure, expected);
            recovery.into_published_session()
                    .stop().unwrap()
                    .finish().unwrap();
            assert!(descriptor.is_cpu_owned());
            assert!(payload.is_cpu_owned());
        }

        for command_failure in [crate::mmc::ReadCommandFailure::Timeout,
                                crate::mmc::ReadCommandFailure::ResponseCrc,
                                crate::mmc::ReadCommandFailure::Io] {
            let (mut descriptor, mut payload) = mappings(transfer);
            let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
            let tracker = published_read_tracker(transfer,
                                                 &mut descriptor,
                                                 &mut payload,
                                                 &mut executor);
            let recovery = match tracker.command_failed(command_failure) {
                crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
                _ => panic!("published command error did not enter recovery"),
            };
            recovery.into_published_session()
                    .stop().unwrap()
                    .finish().unwrap();
            assert!(descriptor.is_cpu_owned());
            assert!(payload.is_cpu_owned());
        }
    }

    #[test]
    fn published_read_dma_failures_and_duplicates_preserve_running_ownership() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        for dma_failure in [crate::mmc::ReadDmaFailure::Start,
                            crate::mmc::ReadDmaFailure::Completion,
                            crate::mmc::ReadDmaFailure::Stop] {
            let (mut descriptor, mut payload) = mappings(transfer);
            let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
            let tracker = published_read_tracker(transfer,
                                                 &mut descriptor,
                                                 &mut payload,
                                                 &mut executor);
            let recovery = match tracker.dma_failed(dma_failure) {
                crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
                _ => panic!("published DMA error did not enter recovery"),
            };
            assert_eq!(recovery.failure,
                       crate::mmc::ReadCompletionFailure::Dma(dma_failure));
            recovery.into_published_session()
                    .stop().unwrap()
                    .finish().unwrap();
            assert!(descriptor.is_cpu_owned());
            assert!(payload.is_cpu_owned());
        }

        for (fact, expected) in
            [(ReadFact::Command, crate::mmc::ReadCompletionFailure::DuplicateCommand),
             (ReadFact::Data, crate::mmc::ReadCompletionFailure::DuplicateData),
             (ReadFact::Dma, crate::mmc::ReadCompletionFailure::DuplicateDma)]
        {
            let (mut descriptor, mut payload) = mappings(transfer);
            let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
            let tracker = published_read_tracker(transfer,
                                                 &mut descriptor,
                                                 &mut payload,
                                                 &mut executor);
            let tracker = match record_read_fact(tracker, fact) {
                crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
                _ => panic!("first completion fact was not pending"),
            };
            let recovery = match record_read_fact(tracker, fact) {
                crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
                _ => panic!("duplicate completion fact did not enter recovery"),
            };
            assert_eq!(recovery.failure, expected);
            recovery.into_published_session()
                    .stop().unwrap()
                    .finish().unwrap();
            assert!(descriptor.is_cpu_owned());
            assert!(payload.is_cpu_owned());
        }
    }

    #[test]
    fn command_observer_feeds_coalesced_snapshot_into_published_tracker() {
        let transfer = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                      Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor);
        let tracker = match tracker.dma_completed() {
            crate::mmc::ReadCompletionProgress::Pending(tracker) => tracker,
            _ => panic!("DMA fact completed read before command/data"),
        };
        let mut observer = crate::mmc::ReadCommandCompletionObserver::new(
            MockMmcCompletionRegisters { interrupts : (1 << 6) | 1,
                                         response : 0x900,
                                         fail_first_read : false,
                                         operations : Vec::new() },
            crate::mmc::ReadCommandObservePermit::fixture(),
            1).unwrap();
        let completed = match tracker.observe_command(&mut observer) {
            crate::mmc::ReadCompletionProgress::Completed(completed) => completed,
            _ => panic!("coalesced CSENT/DFIN did not complete published tracker"),
        };
        assert_eq!(completed.evidence,
                   crate::mmc::ReadCompletionEvidence {
                       command_response_validated : true,
                       data_finished : true,
                       dma_finished : true,
                   });
        assert_eq!(observer.into_inner().operations,
                   [(true, 0x3C, 0),
                    (false, 0x3C, (1 << 6) | 1),
                    (true, 0x14, 0),
                    (false, 0x08, 0),
                    (false, 0x0C, 0)]);
        completed.into_published_session()
                 .stop().unwrap()
                 .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());

        let (mut descriptor, mut payload) = mappings(transfer);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let tracker = published_read_tracker(transfer,
                                             &mut descriptor,
                                             &mut payload,
                                             &mut executor);
        let mut observer = crate::mmc::ReadCommandCompletionObserver::new(
            MockMmcCompletionRegisters { interrupts : 0,
                                         response : 0,
                                         fail_first_read : true,
                                         operations : Vec::new() },
            crate::mmc::ReadCommandObservePermit::fixture(),
            1).unwrap();
        let recovery = match tracker.observe_command(&mut observer) {
            crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery) => recovery,
            _ => panic!("observer IO failure did not preserve published recovery"),
        };
        assert_eq!(recovery.failure,
                   crate::mmc::ReadCompletionFailure::Command(
                       crate::mmc::ReadCommandFailure::Io));
        recovery.into_published_session()
                .stop().unwrap()
                .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn prepared_session_is_returned_when_low_level_executor_is_busy() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let (mut descriptor2, mut payload2) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let prepared = prepare_transfer(plan, &mut descriptor, &mut payload).unwrap();
        executor.start(prepared).unwrap();

        let failure = match prepare_session(plan, &mut descriptor2, &mut payload2).unwrap()
                                                                                       .start(&mut executor) {
            Err(StartSessionFailure::Prepared(failure)) => failure,
            Err(StartSessionFailure::Recovery(_)) => panic!("busy state requires recovery"),
            Ok(_) => panic!("busy executor accepted a second session"),
        };
        assert_eq!(failure.error, ExecutorError::Busy);
        failure.session.cancel().unwrap();
        finish_transfer(executor.stop().unwrap(), &mut descriptor, &mut payload).unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert!(descriptor2.is_cpu_owned());
        assert!(payload2.is_cpu_owned());
    }

    #[test]
    fn quiesced_session_retries_only_mapping_still_owned_by_device() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 512, 16,
                                  Direction::DeviceToMemory).unwrap();
        let descriptor_region = DmaRegion::new(0x4000, 0x2000, 64, 32, 64).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x3000, 512, 32, 64).unwrap();
        let mut descriptor = DmaMapping::new(
            descriptor_region,
            DmaDirection::Bidirectional,
            MockCache { fail_cpu_syncs : 1, ..MockCache::default() });
        let mut payload = DmaMapping::new(payload_region,
                                          DmaDirection::FromDevice,
                                          MockCache::default());
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let completion = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                            .start(&mut executor)
                                                                            .unwrap()
                                                                            .complete_irq(acknowledged_dma_irq()).unwrap();
        // SAFETY: the mock IRQ deterministically marks this transfer stopped.
        let quiesced = unsafe { completion.reclaim_unverified() };
        let failure = quiesced.finish().unwrap_err();
        assert_eq!(failure.error, DriverError::IoError);
        failure.session.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn typestate_stop_quiesces_before_restoring_cpu_ownership() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                               .start(&mut executor).unwrap()
                                                               .stop().unwrap()
                                                               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().writes.last().copied(),
                   Some((plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)));
    }

    #[test]
    fn untouched_start_failure_returns_cancellable_prepared_session() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo { failures : vec![(1, WriteEffect::Untouched)],
                                       ..FaultOrderIo::default() };
        let mut executor = Executor::new(registers, dma_irq());
        let failure = match prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                                .start(&mut executor) {
            Err(failure) => failure,
            Ok(_) => panic!("fault injection unexpectedly started DMA"),
        };
        match failure {
            StartSessionFailure::Prepared(failure) => failure.session.cancel().unwrap(),
            StartSessionFailure::Recovery(_) => panic!("untouched write entered recovery"),
        }
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.stop(), Err(ExecutorError::Idle));
    }

    #[test]
    fn partial_start_write_requires_stop_before_cpu_reclaim() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo { failures : vec![(2, WriteEffect::MayHaveWritten)],
                                       ..FaultOrderIo::default() };
        let mut executor = Executor::new(registers, dma_irq());
        let failure = match prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                                .start(&mut executor) {
            Err(failure) => failure,
            Ok(_) => panic!("fault injection unexpectedly started DMA"),
        };
        let recovery = match failure {
            StartSessionFailure::Recovery(failure) => failure.session,
            StartSessionFailure::Prepared(_) => panic!("partial write remained cancellable"),
        };
        recovery.stop().unwrap()
                .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        let registers = executor.into_inner();
        assert_eq!(registers.writes,
                   vec![0,
                        plan.start_order,
                        (plan.start_order & !ORDER_CONFIG_MASK) | ORDER_64_BIT | (1 << 4)]);
    }

    #[test]
    fn recovery_session_retries_failed_stop_without_releasing_mappings() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo {
            failures : vec![(2, WriteEffect::MayHaveWritten),
                            (3, WriteEffect::MayHaveWritten)],
            ..FaultOrderIo::default()
        };
        let mut executor = Executor::new(registers, dma_irq());
        let failure = match prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                                .start(&mut executor) {
            Err(failure) => failure,
            Ok(_) => panic!("fault injection unexpectedly started DMA"),
        };
        let recovery = match failure {
            StartSessionFailure::Recovery(failure) => failure.session,
            StartSessionFailure::Prepared(_) => panic!("partial write remained cancellable"),
        };
        let failure = match recovery.stop() {
            Err(failure) => failure,
            Ok(_) => panic!("fault injection unexpectedly stopped DMA"),
        };
        assert_eq!(failure.error, ExecutorError::Register);
        failure.session.stop().unwrap()
               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().write_calls, 4);
    }

    #[test]
    fn stop_waits_for_bounded_delayed_confirmation() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo {
            confirmations : vec![Ok(false), Ok(false), Ok(true)],
            ..FaultOrderIo::default()
        };
        let mut executor = Executor::with_stop_poll_limit(registers, dma_irq(), 3).unwrap();
        prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                               .start(&mut executor).unwrap()
                                                               .stop().unwrap()
                                                               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().confirmation_calls, 3);
    }

    #[test]
    fn stop_timeout_keeps_session_recoverable_and_mappings_owned_by_device() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo {
            confirmations : vec![Ok(false), Ok(false)],
            ..FaultOrderIo::default()
        };
        let mut executor = Executor::with_stop_poll_limit(registers, dma_irq(), 2).unwrap();
        let running = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                          .start(&mut executor)
                                                                          .unwrap();
        let failure = match running.stop() {
            Err(failure) => failure,
            Ok(_) => panic!("unconfirmed stop produced a quiesced session"),
        };
        assert_eq!(failure.error, ExecutorError::StopTimeout);
        failure.session.stop().unwrap()
               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        let registers = executor.into_inner();
        assert_eq!(registers.confirmation_calls, 3);
        assert_eq!(registers.writes.len(), 4);
    }

    #[test]
    fn stop_probe_error_keeps_recovery_session_for_retry() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let registers = FaultOrderIo {
            failures : vec![(2, WriteEffect::MayHaveWritten)],
            confirmations : vec![Err(ExecutorError::Register), Ok(true)],
            ..FaultOrderIo::default()
        };
        let mut executor = Executor::with_stop_poll_limit(registers, dma_irq(), 2).unwrap();
        let failure = match prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                                .start(&mut executor) {
            Err(StartSessionFailure::Recovery(failure)) => failure,
            Err(StartSessionFailure::Prepared(_)) => panic!("partial write remained prepared"),
            Ok(_) => panic!("fault injection unexpectedly started DMA"),
        };
        let failure = match failure.session.stop() {
            Err(failure) => failure,
            Ok(_) => panic!("failed confirmation produced a quiesced session"),
        };
        assert_eq!(failure.error, ExecutorError::Register);
        failure.session.stop().unwrap()
               .finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
        assert_eq!(executor.into_inner().confirmation_calls, 2);
    }

    #[test]
    fn executor_rejects_zero_stop_poll_budget() {
        assert!(matches!(Executor::with_stop_poll_limit(MockOrderIo::default(), dma_irq(), 0),
                         Err(ExecutorError::InvalidPollLimit)));
    }

    #[test]
    fn irq_completion_rejects_wrong_acknowledged_source_without_losing_session() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let running = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                          .start(&mut executor)
                                                                          .unwrap();
        let wrong_irq = GlobalIrq::from_bank_local(0, 13).unwrap();
        let wrong = AcknowledgedIrq::after_mask_ack(wrong_irq);
        let failure = match running.complete_irq(wrong) {
            Err(failure) => failure,
            Ok(_) => panic!("wrong IRQ completed APBDMA"),
        };
        assert_eq!(failure.error, ExecutorError::UnexpectedIrq);
        assert_eq!(failure.acknowledged.irq(), wrong_irq);
        let completion = failure.session.complete_irq(acknowledged_dma_irq()).unwrap();
        // SAFETY: the mock IRQ deterministically marks this transfer stopped.
        unsafe { completion.reclaim_unverified() }.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn irq_status_is_read_only_after_descriptor_cpu_sync() {
        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let descriptor_region = DmaRegion::new(0x4000, 0x2000, 64, 32, 64).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x3000, 4, 32, 64).unwrap();
        let mut descriptor = DmaMapping::new(
            descriptor_region,
            DmaDirection::Bidirectional,
            MockCache { fail_cpu_syncs : 1, ..MockCache::default() });
        let mut payload = DmaMapping::new(payload_region,
                                          DmaDirection::ToDevice,
                                          MockCache::default());
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let completion = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                             .start(&mut executor)
                                                                             .unwrap()
                                                                             .complete_irq(acknowledged_dma_irq())
                                                                             .unwrap();
        let mut reader = MockStatusReader { status : 0x100, ..MockStatusReader::default() };
        let failure = match completion.inspect_status(&mut reader, &FixtureStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("status read bypassed failed cache sync"),
        };
        assert_eq!(failure.error, DescriptorStatusError::Cache(DriverError::IoError));
        assert_eq!(reader.calls, 0);
        let (outcome, quiesced) = failure.session
                                           .inspect_status(&mut reader,
                                                           &FixtureStatusDecoder).unwrap();
        assert_eq!(outcome, DescriptorCompletion::Complete);
        assert_eq!(reader.calls, 1);
        quiesced.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }

    #[test]
    fn descriptor_decoder_classifies_fixture_error_and_unknown_status() {
        assert_eq!(FixtureStatusDecoder.decode(0x8000_0042),
                   Ok(DescriptorCompletion::HardwareError(0x8000_0042)));
        assert_eq!(FixtureStatusDecoder.decode(0x42),
                   Err(DescriptorStatusError::Unknown(0x42)));

        let plan = build_transfer(0x2000, 0x3000, 0x1fe2_c040, 4, 1,
                                  Direction::MemoryToDevice).unwrap();
        let (mut descriptor, mut payload) = mappings(plan);
        let mut executor = Executor::new(MockOrderIo::default(), dma_irq());
        let completion = prepare_session(plan, &mut descriptor, &mut payload).unwrap()
                                                                             .start(&mut executor)
                                                                             .unwrap()
                                                                             .complete_irq(acknowledged_dma_irq())
                                                                             .unwrap();
        let mut reader = MockStatusReader { status : 0x42, ..MockStatusReader::default() };
        let failure = match completion.inspect_status(&mut reader, &UnverifiedStatusDecoder) {
            Err(failure) => failure,
            Ok(_) => panic!("unverified decoder claimed completion"),
        };
        assert_eq!(failure.error, DescriptorStatusError::StatusUnverified);
        // SAFETY: the mock IRQ deterministically marks this transfer stopped.
        unsafe { failure.session.reclaim_unverified() }.finish().unwrap();
        assert!(descriptor.is_cpu_owned());
        assert!(payload.is_cpu_owned());
    }
}
