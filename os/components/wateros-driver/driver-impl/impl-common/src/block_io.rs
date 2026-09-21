//! Synchronous block API backed by nonblocking VirtIO submission and completion.
use virtio_drivers::{
    device::blk::{BlkReq, BlkResp, VirtIOBlk},
    transport::Transport,
    Hal,
};

pub fn read<H : Hal, T : Transport>(device : &mut VirtIOBlk<H, T>,
                                    block : usize,
                                    buf : &mut [u8],
                                    irq_wait : bool)
                                    -> virtio_drivers::Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let mut req = BlkReq::default();
    let mut resp = BlkResp::default();
    // Buffers stay on this stack and are not accessed until the matching used entry appears.
    let token = unsafe { device.read_blocks_nb(block, &mut req, buf, &mut resp)? };
    if irq_wait {
        crate::irq::wait_until(&mut || device.peek_used() == Some(token));
    } else {
        while device.peek_used() != Some(token) {
            core::hint::spin_loop();
        }
    }
    let result = unsafe { device.complete_read_blocks(token, &req, buf, &mut resp) };
    // IRQ-off callers cannot run the hard handler; clear the completed device level here.
    device.ack_interrupt();
    result
}

pub fn write<H : Hal, T : Transport>(device : &mut VirtIOBlk<H, T>,
                                     block : usize,
                                     buf : &[u8],
                                     irq_wait : bool)
                                     -> virtio_drivers::Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let mut req = BlkReq::default();
    let mut resp = BlkResp::default();
    // The caller's immutable buffer and request/response remain live through completion.
    let token = unsafe { device.write_blocks_nb(block, &mut req, buf, &mut resp)? };
    if irq_wait {
        crate::irq::wait_until(&mut || device.peek_used() == Some(token));
    } else {
        while device.peek_used() != Some(token) {
            core::hint::spin_loop();
        }
    }
    let result = unsafe { device.complete_write_blocks(token, &req, buf, &mut resp) };
    device.ack_interrupt();
    result
}
