# Task 08 中间记录：RV SIGSEGV 候选修复（Sv39 默认关闭 elf-lazy-map）

## 背景

RV `-smp 8` BuildStorm 在当前 main baseline 上随机出现 guest StorePageFault
（如 `fault_addr=0x70022b40`），WaterOS 内核以 `SIGSEGV signal not delivered`
杀掉用户任务。同一镜像跑 baseline 内核同样复现，确认不是 slab 引入。

故障地址位于动态链接器固定基址 `0x70000000` 附近；Sv39 默认开启
`elf-lazy-map`，解释器和私有匿名映射都走 lazy VMA + 缺页加载，疑点集中在 lazy
VMA 注册/合并/缺页路径。

## 候选修复

把 Sv39 `impl-sv39` 的默认 feature 从 `[ "api-v0", "elf-lazy-map" ]` 改为
`[ "api-v0" ]`，即 RV 默认退回 eager ELF/匿名映射。该改动不改变 Linux 语义，
只是把“缺页时加载”改成“映射时加载”，同时保留 `elf-lazy-map` feature 可重新开启。

## 状态

- `make rv_check` / `make la_check` / `make kernel-rv-final` 通过；
- cagent 与 buildstorm 启动段正常；
- 完整 RV `-smp 8` BuildStorm 尚未在无外部 QEMU 干扰窗口内跑完，**仍需最终验证**；
- 若验证通过，再决定是否长期保留或回退到修复 lazy VMA 根因。
