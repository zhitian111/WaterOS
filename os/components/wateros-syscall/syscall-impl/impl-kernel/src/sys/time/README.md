# time syscall

本目录实现 wall/monotonic 时钟、睡眠、interval timer、POSIX timer、timerfd 和 RTC。

## 当前能力

- gettimeofday、clock_gettime/getres/settime、clock_nanosleep 与 nanosleep。
- timer_create/settime/gettime/getoverrun/delete、getitimer/setitimer。
- timerfd realtime/monotonic、绝对/相对、周期累计、poll、dup/fork 共享状态。
- BSP 是全局 timeout timekeeper；AP timer 只处理本 CPU 时间片，SMP 不会加速超时。

## 已知边界

`TFD_TIMER_CANCEL_ON_SET`、time namespace、高精度亚 tick timer 和完整 CPU clock 尚未实现。
扩展时应使用统一 clock source/deadline，不能在每 CPU timer 上重复推进同一全局定时器。
