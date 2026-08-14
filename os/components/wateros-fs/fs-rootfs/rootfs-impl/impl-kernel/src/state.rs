//! Root-volume generation state used to invalidate dependent caches.

use core::sync::atomic::{AtomicU64, Ordering};

static MOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn mount_generation() -> u64 {
    MOUNT_GENERATION.load(Ordering::Acquire)
}

pub fn bump_mount_generation() {
    MOUNT_GENERATION.fetch_add(1, Ordering::Release);
}

pub(crate) fn next_mount_generation() {
    bump_mount_generation();
}
