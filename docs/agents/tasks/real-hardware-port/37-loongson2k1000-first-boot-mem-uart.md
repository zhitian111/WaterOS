# 37 Loongson 2K1000 首启：UART 高窗口 + DMW + uImage 瘦身

## 任务内容

为 2K1000 真机首启修正地址模型（对照 NPUcore 与真机 U-Boot 2022.04 证据）：

1. UART0 走未缓存 MMIO 高窗口：`0x8000_0000_0000_0000 | 0x1FE2_0000`，
   而非裸 `0x1FE2_0000`；
2. 不覆盖 U-Boot 已配好的 DMW0（VSEG=0→DRAM）；删除 `init_trap` 里错误的
   `DMW0=0x11`；
3. LA uImage 剥离 `.kernel.heap`，避免 256 MiB 镜像。

## 实施方案

1. `impl-loongson2k1000la/console.rs`：`UART_BASE` 改为高窗口。
2. `impl-loongarch64/trap.rs`：移除 `write_csr(DMW0, 0x11)` 及无用常量。
3. `os/Makefile` `la2k_uimage`：objcopy 增加 `--remove-section=.kernel.heap`。
4. `os/scripts/root_image/mk_uimage.py`：修正 legacy uImage 架构号；
   板级 U-Boot 使用 Loongson vendor 的 `IH_ARCH_LA=27`（不是 24/x86_64），
   RISC-V 同步从 22 修正为上游的 26。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-platform/platform-impl/impl-loongson2k1000la/src/console.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs`
- `os/Makefile`
- `os/scripts/root_image/mk_uimage.py`

CodeGraph：

```bash
codegraph explore "console_write_a_byte"
codegraph explore "init_trap"
```

## 验收方式

- [x] `make la2k_check` / `make la_check` 通过。
- [x] `make la2k_uimage` 产出小体积 uImage（~2 MiB），load/entry 0x90000000，
      且 legacy 头架构号为 27（vendor `IH_ARCH_LA`）。
- [ ] 真机 TFTP 启动后 UART 打印内核 banner。

## 验收命令

```bash
cd os
make la2k_check
make la2k_uimage
ls -la kernel-la2k.bin kernel-la2k.ui
mkimage -l kernel-la2k.ui | head
git diff --check
```

## 验证环境

- L0 宿主机：check/build。✅
- L3 真机：TFTP bootm → 串口 banner。🔴

## 任务简报

## 任务简报

- 完成日期：2026-08-16
- commit：见工作区提交
- 实际改动：UART0 高窗口、不覆盖 DMW0、剥离 `.kernel.heap`、修正 LoongArch
  uImage 架构号为 27。
- 验收结果：L0 `make la2k_check` / `make la_check` / `make la2k_uimage` 通过；
  L3 TFTP bootm 仍未验证，等板端串口/网络命令确认。
- 未验证项：真机入口、MMU 启用后的 UART 映射、以及是否需要额外 DTB/参数。
