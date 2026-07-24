//! Architecture-neutral shape of a local TLB invalidation request.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlbFlushRange {
    All,
    AddressSpace { token: usize },
    Page { addr: usize },
    Range { start: usize, end: usize },
}
