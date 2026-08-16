# 50 Loongson 2K1000 CRMD 前后探针

## 任务内容

高地址链接后仍只输出 `W`。为判断是 `CRMD.DA` 切换前还是切换后挂住，
在 `_start.S` 增加：

1. `A`：准备切 CRMD 前
2. `B`：`csrwr CRMD` 完成后

## 涉及文件

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出揭示卡在 `A` 前、`AB` 之间，或 `AB` 之后

## 任务简报

- 完成日期：2026-08-16
- CRMD 探针已加入；等待板端串口。
