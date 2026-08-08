//! IO 多路复用相关的 syscall 实现。

pub(crate) mod epoll;
pub(crate) mod poll;
pub(crate) mod poll_multiplex;

pub(crate) use epoll::{
    sys_epoll_create1, sys_epoll_ctl, sys_epoll_pwait, sys_epoll_pwait2, sys_epoll_wait,
};
pub(crate) use poll::sys_poll;
pub(crate) use poll_multiplex::{sys_ppoll, sys_pselect6, sys_select};
