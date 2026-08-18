# RTC 字符设备路由 stub 手册

这个 crate 容易被误解：它不读取 RTC 硬件，只注册一个 `CharacterDeviceKind::Rtc` 标记，让
VFS/syscall 将 RTC ioctl 路由到平台 wall-clock。字符接口见
[character-api](../../character-api/api-v0/README.md)，时间 syscall 说明见
[sys/time](../../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/time/README.md)。

## 1. 当前数据结构与行为

`RtcCharacterDevice` 是无状态零大小标记类型：read 返回 `Ok(0)`，write 返回
`DriverError::Unsupported`，kind 返回 `Rtc`，trait ioctl 默认也返回 Unsupported。

`RtcTime` 是九个 `i32` 字段的 `#[repr(C)]` Linux 风格日历结构，但当前实际 ioctl 路径在
syscall crate 内使用自己的 `UserRtcTime` 做 user-copy。不要看到公开 `RtcTime` 就直接在驱动
里解引用 ioctl `arg`；用户指针永远应由 syscall/user-copy 层处理。

字段约定：month 通常为 0..11，year 为自 1900 起，weekday/year-day 使用 libc/Linux 约定；
最终转换规则以 `platform::wall_clock::{ns_to_rtc_time,rtc_time_to_ns}` 为准。

## 2. 真实调用链

```text
bring-up
  -> register_builtin_character_devices
  -> register_rtc_stub
  -> character registry 中加入 kind=Rtc 的对象
  -> devfs manager 创建 /dev/rtc* 与 /dev/misc/rtc alias

ioctl(rtc_fd, RTC_RD_TIME/RTC_SET_TIME, user_ptr)
  -> VFS/fd-session 识别该 fd 的 CharacterDeviceKind::Rtc
  -> syscall ioctl 分发到 sys_rtc_ioctl
  -> user-copy UserRtcTime
  -> platform::wall_clock 读取/设置 ns 或转换日历字段
  -> errno / 用户结构返回
```

因此 stub 的存在是“设备身份与路径锚点”。如果 alias 正常但 ioctl 返回异常，应先查 syscall
分发和 platform wall clock，而不是给 `RtcCharacterDevice::read` 填逻辑。

## 3. ABI、权限和锁

RTC ioctl 编号含方向与结构大小；syscall 必须只接受支持的 request。`RTC_SET_TIME` 是全局
时钟变更，应在 syscall/credential 层检查能力或 root 权限，并验证年月日范围、闰年和算术
溢出。驱动注册表 mutex 不能跨越 user-copy 或平台时钟调用。

RTC 字符对象无内部锁状态；真实 wall clock 的并发与硬件/firmware 访问由 platform 层负责。
读取时间不应依赖任务本地 timezone，内核通常维护 UTC，用户空间负责时区显示。

## 4. 接入真实硬件 RTC 的方案

若以后支持 Goldfish/CMOS/I2C RTC，不应直接把 MMIO 访问塞进这个 stub：

1. 在 platform 或专用 RTC driver 定义稳定的 read/set time trait；
2. 明确硬件寄存器锁、BCD 转换、snapshot/latch 和更新中状态；
3. 平台枚举选择实现并注册 RTC kind；
4. syscall 继续负责 Linux `rtc_time` user-copy、权限与 errno；
5. `RTC_RD_TIME` 调用硬件 trait，失败映射 `EIO`；
6. `RTC_SET_TIME` 只在硬件可写且权限通过时提交，避免部分字段更新；
7. 无硬件时可保留基于 platform wall clock 的虚拟 RTC，但文档和 capability 必须明确。

若需要 alarm/periodic IRQ，必须增加等待队列、poll readiness 和事件计数；当前永远 EOF 的 read
不具备 `/dev/rtc` 中断事件 ABI。

## 5. 常见故障

- `hwclock` 报 ENOTTY：fd kind 没被识别为 Rtc，或 ioctl 未转发到 time 模块；
- `/dev/rtc` 不存在：builtin 未注册，或 devfs alias 扫描在注册前执行；
- 读取恒为 1970/错误年月：查 platform wall clock 与字段 epoch 转换；
- 设置时间无权限也成功：cred 检查遗漏；
- 直接 read 得到 EOF：这是当前设计，不是硬件故障；工具应走 ioctl；
- 多个 RTC alias 指向不稳定：检查重复注册与 devfs kind/index 选择。

## 6. 回归清单

验证 RTC kind 和 alias、`RTC_RD_TIME` 字段、闰年/月末、非法日期、`RTC_SET_TIME` 权限、无效
用户指针、未知 ioctl、并发读写以及 RV/LA platform wall-clock。read=EOF/write=Unsupported 也要
保留测试，避免上层误把 stub 当作事件流设备。

