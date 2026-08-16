# 53 Loongson 2K1000 早期改用 board console 直写

## 任务内容

`T` 探针证明已进入 Rust，但 `platform::console` 聚合层调用未输出。
为绕过 console 锁/中断屏蔽等早期路径，先把 2K1000 入口的第一条日志
改为直接调用 `platform::active_impl::console::console_write_raw_buffer`。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出出现 `WRST[2K1000] enter WaterOS Rust`

## 任务简报

- 完成日期：2026-08-16
- 已改用 board console 直写；等待板端串口。
