use core::sync::atomic::{AtomicUsize, Ordering};
static DTB_PA : AtomicUsize = AtomicUsize::new(0);
pub fn store(dtb_pa : usize) { DTB_PA.store(dtb_pa, Ordering::Release); }
pub fn dtb_pa() -> usize { DTB_PA.load(Ordering::Acquire) }
