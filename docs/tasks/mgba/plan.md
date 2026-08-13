# mGBA 执行计划

## M0：上游锁定与最小构建

- [ ] 获取 `mgba-emu/mgba`，记录 tag、commit、获取日期、许可证和 SHA-256；
- [ ] 审查当前版本 CMake feature 开关与静态 musl 交叉编译需求；
- [ ] 关闭 Qt、SDL、FFmpeg、OpenGL、libzip、debugger UI、网络及音频输出；
- [ ] 确定仅构建 core 和自定义 frontend 的 target。

验收：干净的 riscv64 Linux 静态 ELF 可以链接，且没有动态 `PT_INTERP` 或 `NEEDED`。

## M1：确定性 core frontend

- [ ] 实现 `water-mgba --frames N --no-video --no-audio ROM.gba`；
- [ ] 打印 ROM 信息、帧数、CPU/RAM/VRAM/framebuffer hash；
- [ ] 在 x86_64 Linux 与 riscv64 Linux 比较固定帧数结果。

验收：同一 ROM 的 hash 一致；失败时输出最早分歧帧。

## M2：WaterOS 无窗口运行

- [ ] 把 frontend 纳入 `user/packages/mgba`，写入 Nano-X profile 或独立 profile；
- [ ] 记录并验证 ROM loader、分配、时间和保存路径所需 syscall；
- [ ] WaterOS 运行 1,000 帧，与 M1 reference 比对 hash。

验收：无画面连续完成，不 crash、不卡死，hash 一致。

## M3：Nano-X 视频和键盘

- [ ] 创建 240×160 Nano-X 窗口，先使用直接像素 blit；
- [ ] 明确 mGBA framebuffer 与 Nano-X 颜色格式的转换；
- [ ] 映射方向键、X/Z、A/S、Enter、Backspace；处理 KeyDown 与 KeyUp。

验收：可见正确首帧，基本游戏输入正常，不出现粘键。

## M4：帧同步、存档和稳定性

- [ ] 基于 `CLOCK_MONOTONIC` 和绝对 deadline 做 59.7275 FPS pacing；
- [ ] 编写 sleep 1/5/10/16/100 ms 的用户态测量程序；
- [ ] 验证 `.sav` 的创建、更新、退出、重启恢复；
- [ ] 运行至少 30 分钟，记录 FPS、内存、卡死和漂移。

验收：速度可接受、保存可恢复、无明显资源增长或累计帧漂移。

## 音频（后续，不阻塞 M0–M4）

当前明确不实现。未来单独设计 PCM ring buffer 和设备 backend，不能在模拟线程同步等待播放。
