//! 网络相关的 syscall 实现。

pub(crate) mod accept;
pub(crate) mod bind;
pub(crate) mod connect;
pub(crate) mod listen;
pub(crate) mod recvfrom;
pub(crate) mod sendmsg;
pub(crate) mod sendto;
pub(crate) mod shutdown;
pub(crate) mod socket;
pub(crate) mod socketpair;
pub(crate) mod sockname;
pub(crate) mod sockopt;

pub(crate) use accept::{sys_accept, sys_accept4};
pub(crate) use bind::sys_bind;
pub(crate) use connect::sys_connect;
pub(crate) use listen::sys_listen;
pub(crate) use recvfrom::sys_recvfrom;
pub(crate) use sendmsg::{sys_recvmsg, sys_sendmsg};
pub(crate) use sendto::sys_sendto;
pub(crate) use shutdown::sys_shutdown;
pub(crate) use socket::sys_socket;
pub(crate) use socketpair::sys_socketpair;
pub(crate) use sockname::{sys_getpeername, sys_getsockname};
pub(crate) use sockopt::{sys_getsockopt, sys_setsockopt};
