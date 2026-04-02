#[allow(unused)]
pub trait SyscallArgRead {
    const SYSCALL_ARG_CAP : usize;
    fn syscall_nr(&self) -> usize;
    fn arg(&self, idx : usize) -> Option<usize>;
}


#[allow(unused)]
pub trait SyscallArgWrite {
    fn set_syscall_ret(&mut self, ret : isize);
}

#[allow(unused)]
pub trait SyscallFrameView: SyscallArgRead + SyscallArgWrite {}
