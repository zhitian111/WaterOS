# 14 JH7110 内核构建与 U-Boot uImage 打包

## 任务内容

让 VisionFive 2 的「内核可引导」这一步可复现：构建 `jh7110-visionfive2`
板级内核，objcopy 成 flat binary，再打包成 U-Boot legacy uImage（入口
`0x40200000`），并产出启动分区素材目录（uImage + `extlinux.conf` +
`uEnv.txt` + `boot.scr`）。

这是真机启动的前置里程碑（任务 08b 之前的"内核上板"第一步）。rootfs 与
userspace-init 已在任务 12/13 完成；本任务不做真机烧录（后置任务 15）。

## 实施方案

1. `os/Makefile` 增加 `kernel-jh7110`：以
   `--no-default-features --features jh7110-visionfive2,pre` +
   `--target riscv64gc-unknown-none-elf` 构建，产物复制为
   `os/kernel-jh7110`（ELF，与 `kernel-la2k` 同款结构）。
2. 增加 `jh7110_uimage`：
   - `riscv64-linux-gnu-objcopy -O binary --strip-all` 产出
     `kernel-jh7110.bin`（**剔除 `.kernel.heap` 段**：静态堆是 256 MiB
     的 PROGBITS 零区，TLSF 分配器 `insert_free_block_ptr` 自建元数据、
     不依赖清零，剔除后 uImage 从 271 MiB 降到 ~2 MiB）；
   - 优先 `mkimage -A riscv -O linux -T kernel -C none -a 0x40200000
     -e 0x40200000`；mkimage 缺失时回退到
     `os/scripts/root_image/mk_uimage.py --arch riscv`（本任务给该脚本
     增加 `--arch` 参数，RISC-V 架构字节为 22）。
3. 增加 `jh7110_bootdir`：把启动素材组装到 `os/build/jh7110-boot/`：
   - `wateros-jh7110.ui`（uImage 改名后的稳定文件名）；
   - `boot.scr`（由 `os/scripts/root_image/boot-visionfive2.cmd` 经
     `mkimage -T script` 生成，U-Boot 标准 script 扫描路径）；
   - `extlinux/extlinux.conf`（模板 `jh7110-extlinux.conf`，与官方镜像
     同路径，出厂 U-Boot `bootcmd_distro` 直接 sysboot 它）；
   - `uEnv.txt`（模板 `jh7110-uEnv.txt`，只设地址变量，保持出厂
     `load_distro_uenv` 的 env import 干净）。
4. 板级 DTB 由使用者提供（Linux GPL 产物，不入库），缺省时 `boot.scr`
   回退到 `$fdtcontroladdr`。
5. **补齐板级 bring-up 接线**（构建时发现的两个缺口，均属本任务范围）：
   - `os/src/main.rs`：新增共享入口模块 `riscv64_opensbi_entry`
     （`cfg(any(qemu-riscv64-opensbi, jh7110-visionfive2))`）——此前
     `jh7110_check` 只 check 不链接，`wateros_kernel_main` 在板级 feature
     下未定义，链接必挂；两个平台同为 OpenSBI + a0=hart/a1=DTB + SBI HSM，
     入口逻辑完全一致，不复制代码。
   - `os/build.rs`：补 `jh7110-visionfive2` 的 `-T .../link.ld` 链接脚本
     登记——此前板级构建没有链接脚本，`kernel_end`/`kernel_heap_*` 未定义。

## 涉及文件 / CodeGraph 查询

- `os/Makefile`
- `os/src/main.rs`（共享 RISC-V64 OpenSBI 入口模块）
- `os/build.rs`（jh7110 链接脚本接线）
- `os/scripts/root_image/mk_uimage.py`（新增 `--arch`）
- `os/scripts/root_image/boot-visionfive2.cmd`（新增）
- `os/scripts/root_image/jh7110-extlinux.conf`（新增）
- `os/scripts/root_image/jh7110-uEnv.txt`（新增）
- `os/.gitignore`（新增内核产物与 `/build/`）

CodeGraph：

```bash
codegraph explore "KERNEL_ENTRY_ADDRESS"
codegraph explore "boot"     # impl-jh7110-visionfive2/src/boot.rs：a0/a1 传参
codegraph explore "console_write_a_byte"
```

## 验收方式

- [ ] `make jh7110_check` 通过（板级 feature 全量 check）。
- [ ] `make jh7110_uimage` 产出 `kernel-jh7110.ui`，`mkimage -l` 显示
      RISC-V Linux Kernel Image、Load/Entry = 0x40200000。
- [ ] `make jh7110_bootdir` 产出 `os/build/jh7110-boot/` 四件套；
      `mkimage -l boot.scr` 显示 U-Boot script。
- [ ] ELF 入口地址与 link.ld 的 `0x40200000` 一致。
- [ ] `make rv_check` 无回归（入口模块为 qemu/jh7110 共享）。
- [ ] `git diff --check` 干净；无 GPL 文件入库。

## 验收命令

```bash
cd os
make jh7110_check
make jh7110_uimage
mkimage -l kernel-jh7110.ui
readelf -h kernel-jh7110 | grep -E "Entry|Machine"
make jh7110_bootdir
mkimage -l build/jh7110-boot/boot.scr
make rv_check
git diff --check
```

## 验证环境

- L0 宿主机：构建 + 镜像头/入口校验。✅（本任务全量可验）
- L1 QEMU virt：不适用（JH7110 内存布局与 virt 不同；公共层回归由
  `make rv_check` 覆盖）。
- L3 真机：U-Boot 实际加载 uImage 并以 a0/a1 进入内核（后置任务 15
  烧录 + 08b 串口日志）。

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - `os/src/main.rs`：`qemu_riscv64_opensbi` 入口模块改名为共享的
    `riscv64_opensbi_entry`，`cfg(any(feature = "qemu-riscv64-opensbi",
    feature = "jh7110-visionfive2"))`；两平台同为 a0=hart/a1=DTB + SBI HSM。
  - `os/build.rs`：补 jh7110 的 link.ld/-T 与 `_start.S` rerun 登记。
  - `os/Makefile`：新增 `kernel-jh7110`、`jh7110_uimage`、`jh7110_bootdir`
    目标；uImage 用 objcopy `--remove-section=.kernel.heap` 剔除 256 MiB
    静态堆零区（TLSF 不依赖清零），271 MiB → 1.94 MiB。
  - `os/scripts/root_image/mk_uimage.py`：新增 `--arch {loongarch,riscv}`
    （RISC-V 架构字节 22），mkimage 缺失时的回退路径。
  - 新增启动模板：`boot-visionfive2.cmd`、`jh7110-extlinux.conf`、
    `jh7110-uEnv.txt`；`os/.gitignore` 补 `kernel-jh7110*` 与 `/build/`。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - `make jh7110_uimage`：`mkimage -l` 显示 RISC-V Linux Kernel Image，
    Load/Entry = 0x40200000，Data Size 2038272 B（1.94 MiB）。
  - `readelf -h kernel-jh7110`：Entry point 0x40200000。
  - `make jh7110_bootdir`：boot.scr（U-Boot script 532 B）与
    extlinux/uEnv 模板齐全；板级 DTB 从官方镜像抽出放
    `os/build/jh7110-boot/`（GPL 产物，仅本地，不入库）。
  - `git diff --check`：clean。
- 未验证/风险：
  - 真机 U-Boot 实际加载与 a0/a1 传参（后置任务 15 烧录 + 08b）。
  - `kernel-la2k` 的 uImage 同样包含 256 MiB 堆段（约 271 MiB），本任务
    未改 LA 路径，留给任务 09/10 构建时按同样方式处理。
  - 静态堆不在镜像中，依赖 U-Boot 加载后该区域 RAM 可用且分配器自建
    元数据；首次真机启动时重点观察堆初始化日志。
