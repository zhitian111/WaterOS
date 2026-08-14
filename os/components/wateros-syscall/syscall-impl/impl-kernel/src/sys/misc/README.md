# misc syscall

本目录收纳不属于单一对象域的系统控制接口。

## 当前能力

- uname/sethostname/setdomainname、sysinfo、getrandom。
- mount/umount2、sync/syncfs、reboot、acct 与内核 syslog。
- RISC-V icache flush/hwprobe、RTC ioctl 分流和 bringup 诊断统计。
- sysinfo 已接 frame allocator 与 process registry，不再使用固定空闲内存/进程数。

## 已知边界与扩展

- getrandom 当前没有硬件熵源支撑，不能声明密码学安全；应接 VirtIO RNG 或板载 RNG，
  用经过审计的 DRBG，并统一 `/dev/urandom` 状态。
- mount/reboot/hostname 权限目前以 root 近似 capability。
- load average、buffer/cache/shared RAM 仍需 scheduler 与页缓存精确计数。
- 模块加载、swap 和完整 namespace 控制保持 `ENOSYS`，避免静态组件架构下假成功。
