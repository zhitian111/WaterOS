use water_os::init_allocator;
use water_os::kernal_log;
extern crate alloc;
const HEAP_SIZE : usize = 1024 * 1024;

#[unsafe(link_section = "bss.heap")]
static mut HEAP : [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[unsafe(no_mangle)]
pub extern "C" fn init_virtual_memory() {
    init_allocator(core::ptr::addr_of_mut!(HEAP) as *mut u8,
                   HEAP_SIZE);
    kernal_log!("Heap initialized at {:x} with size {} bytes.",
                core::ptr::addr_of_mut!(HEAP) as usize,
                HEAP_SIZE);
    let mut vec = alloc::vec::Vec::<u32>::new();
    vec.push(32);
    kernal_log!("Vec created with the value {}.", vec[0]);
    kernal_log!("Allocker test passed.")
}
