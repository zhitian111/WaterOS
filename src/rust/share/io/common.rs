use core::ptr;
pub fn read_value_at_address<T : Copy>(address : usize, offset : usize) -> T {
    unsafe {
        let ptr = (address + offset) as *const T;
        return ptr::read_volatile(ptr);
    }
}
