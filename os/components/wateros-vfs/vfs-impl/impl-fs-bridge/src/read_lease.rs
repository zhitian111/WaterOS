//! Shared prepared-read reservation and staged lease support.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::{
    VfsCopyProgress, VfsError, VfsOpenDescriptionState, VfsReadFinish, VfsReadLease,
    VfsReadReservation, VfsResult,
};

pub(crate) struct ReservationGuard {
    description : Arc<VfsOpenDescriptionState>,
    reservation : Option<VfsReadReservation>,
}

impl ReservationGuard {
    pub(crate) fn begin(description : Arc<VfsOpenDescriptionState>) -> VfsResult<Self> {
        let reservation = description.begin_read()?;
        Ok(Self { description,
                  reservation : Some(reservation) })
    }

    pub(crate) fn offset(&self) -> u64 {
        self.reservation
            .expect("active read reservation")
            .offset()
    }

    pub(crate) fn retarget(&mut self, offset : u64) -> VfsResult<()> {
        let reservation = self.reservation.ok_or(VfsError::Io)?;
        self.reservation = Some(self.description.retarget_read(reservation, offset)?);
        Ok(())
    }

    pub(crate) fn commit(&mut self, copied : usize, staged : usize) -> VfsResult<()> {
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        self.description.finish_read(reservation, copied, staged)?;
        Ok(())
    }

    pub(crate) fn commit_at(&mut self, offset : u64) -> VfsResult<()> {
        let reservation = self.reservation.take().ok_or(VfsError::Io)?;
        self.description.finish_read_at(reservation, offset)?;
        Ok(())
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = self.description.cancel_read(reservation);
        }
    }
}

pub(crate) struct StagedReadLease {
    reservation : ReservationGuard,
    data : Vec<u8>,
}

impl StagedReadLease {
    pub(crate) fn new(reservation : ReservationGuard, data : Vec<u8>) -> Self {
        Self { reservation,
               data }
    }
}

impl VfsReadLease for StagedReadLease {
    fn bytes(&self) -> &[u8] { self.data.as_slice() }

    fn len(&self) -> usize { self.data.len() }

    fn visit(&self, visitor : &mut dyn FnMut(&[u8]) -> bool) {
        let _ = visitor(self.data.as_slice());
    }

    fn finish(mut self : Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied > self.data.len() {
            return Err(VfsError::Io);
        }
        self.reservation.commit(progress.copied, self.data.len())?;
        if progress.copied == 0 && !progress.complete {
            Ok(VfsReadFinish::Fault)
        } else {
            Ok(VfsReadFinish::Bytes(progress.copied))
        }
    }
}

pub(crate) fn try_zeroed(len : usize) -> VfsResult<Vec<u8>> {
    let mut data = Vec::new();
    data.try_reserve_exact(len).map_err(|_| VfsError::NoMemory)?;
    data.resize(len, 0);
    Ok(data)
}

pub(crate) fn allocation_failure_self_test() -> VfsResult<()> {
    let description = Arc::new(VfsOpenDescriptionState::new(0, 0));
    let reservation = ReservationGuard::begin(description.clone())?;
    if try_zeroed(usize::MAX) != Err(VfsError::NoMemory) {
        return Err(VfsError::Io);
    }
    drop(reservation);
    if description.offset() != 0 || description.read_reservation_active() {
        return Err(VfsError::Io);
    }
    Ok(())
}
