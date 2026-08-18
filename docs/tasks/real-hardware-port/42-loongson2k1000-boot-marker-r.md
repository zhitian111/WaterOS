# 42 Loongson 2K1000 boot 栈后探针

## 任务内容

板端只输出 `W`，仍未进入 Rust。继续缩小范围：在 `__wateros_arch_boot`
完成 boot-stack 设置、即将 `bl wateros_kernel_main` 前，通过弱符号
平台 hook 输出 `R`。

## 涉及文件

- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/asm/boot.S`
- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/asm/_start.S`

## 验收方式

- [x] `make la2k_check` / `make la2k_uimage` 通过
- [x] 新内核已更新到 TFTP
- [ ] 板端输出 `WR`；若只有 `W` 则卡在 boot-stack 计算；若 `WR`
      仍无 Rust 消息则卡在 `bl wateros_kernel_main`/Rust 入口前

## 任务简报

- 完成日期：2026-08-16
- 新增 `R` 探针；等待板端串口。
