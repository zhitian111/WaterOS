# Task 08 简报：最终功能验收（进行中）

## 状态

进行中。LA `-smp 12` 多次完整通过；RV `-smp 8` 被当前 main 既有 guest SIGSEGV
阻塞，已提交两项候选修复，等待稳定 QEMU 窗口做完整验证。

## 已提交修复

- `9f3ab46f`：Sv39 默认关闭 `elf-lazy-map`（保留 feature，Linux 语义不变）；
- `a113f3ab`：lazy VMA 二分查找增加线性回退，防止失序漏查误报 SIGSEGV。

## 已验证

- `make rv_check` / `make la_check` / `make kernel-rv-final` 通过；
- LA 12 核性能/功能多轮通过；
- RV 单核完整 buildstorm 通过；
- RV 8 核完整 BuildStorm 仍需在无外部 QEMU 干扰窗口验证。
