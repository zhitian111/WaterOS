# 62 Loongson 2K1000 绕过早期 info! 日志

## 任务内容

M17 后未到 M18，中间包含 `info!`。首启阶段日志宏仍会走
`platform::console` 聚合层，先替换为 board console 直写。

## 涉及文件

- `os/src/main.rs`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出越过 M18

## 任务简报

- 完成日期：2026-08-16
- 已绕过早期 info!；等待板端串口。
