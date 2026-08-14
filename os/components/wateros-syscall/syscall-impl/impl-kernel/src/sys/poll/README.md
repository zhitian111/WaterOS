# poll syscall

本目录提供 poll/ppoll、select/pselect6 和 epoll 系列。`poll_engine.rs` 统一完成 fd
扫描、tick deadline、信号 mask 临时替换和阻塞重扫。

## 当前能力

- 普通 VFS fd、pipe、TTY/PT​​Y、eventfd/timerfd/signalfd/inotify、socket 与 pidfd 就绪。
- epoll create/ctl/wait/pwait/pwait2，支持 LT/ET、ONESHOT 与用户 data。
- 阻塞时使用独立句柄或短 waitqueue，不跨 fd 槽锁调度；信号可返回 EINTR。

## 扩展方向

当前多 fd 等待以至多一个 tick 的重扫保证不同对象间不会丢唤醒，正确但高并发成本较高。
后续可建立统一 poll subscription token，把一次 waiter 同时注册到多个对象，并在 close、
dup、epoll 嵌套和对象销毁时安全撤销。
