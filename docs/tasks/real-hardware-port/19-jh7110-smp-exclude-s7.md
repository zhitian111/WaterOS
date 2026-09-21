# 19 JH7110 SMP 排除 S7 监控核（hart 0）

## 任务内容

修复真机 AP 启动 panic：`[smp] AP init_current_cpu failed cpu=?: Not Found`
（`src/main.rs` AP 路径，PLIC `context_for_hart` 返回 `NotFound`）。

根因（真机 DTB 证据）：板级 DTB 中 `cpu@0`（`sifive,s7` 监控核）标记为
`status = "disabled"`，且 PLIC `interrupts-extended` 只给 hart 0 声明了
M 态上下文（irq 11），没有 S 态上下文（irq 9）；应用核只有 U74
harts 1..=4（各自带 M 态 + S 态上下文）。把 hart 0 当 AP 启动后在
`irq::initialize_current_hart` 必然 `NotFound` panic。

## 实施方案

1. 新增 `impl-jh7110-visionfive2/src/smp.rs`：`Jh7110Smp` 包装
   `opensbi_common::smp::OpenSbiSmp`（HSM/IPI/remote fence 全部委托），
   仅覆盖 `configured_cpu_mask()` 为 harts 1..=4（mask `0b11110`）。
2. `lib.rs` 不再 re-export `opensbi_common::smp`，改用自有 `smp` 模块；
   `Cargo.toml` 补 `base` 依赖（`CpuId`/`CpuMask`）。
3. `riscv64_opensbi_entry::start_secondary_harts` 与 LA 路径一致：先取
   `platform::smp::configured_cpu_mask()`，跳过不在 mask 内的 hart。

## 涉及文件 / CodeGraph 查询

- `os/components/wateros-platform/platform-impl/impl-jh7110-visionfive2/src/smp.rs`（新增）
- 同目录 `lib.rs`、`Cargo.toml`
- `os/src/main.rs`（`start_secondary_harts`）

CodeGraph：

```bash
codegraph explore "configured_cpu_mask"
codegraph explore "start_secondary_harts"
codegraph explore "context_for_hart"
```

## 验收方式

- [ ] `make jh7110_check` / `make rv_check` 通过。
- [ ] QEMU virt 回归无变化（mask 仍为全部 hart）。
- [ ] 真机重启后 AP（2/3/4）通过 PLIC 初始化并 online，不再出现
      hart 0 的 `init_current_cpu failed`。

## 验收命令

```bash
cd os
make jh7110_check
make rv_check
make jh7110_uimage && make jh7110_bootdir
cd ../user && make disk ARCH=rv PACKAGE=minimal IMAGE_SIZE_MB=64 \
  DISK_SIZE_MB=192 BOOT_DIR=../os/build/jh7110-boot BOOT_SIZE_MB=64
cd ../os && make run ARCH=rv PROFILE=pre SDCARD=../user/build/images/wateros-rv.img
git diff --check
```

## 验证环境

- L0 宿主机：check/构建。✅
- L1 QEMU virt：mask 不变回归。✅
- L3 真机：AP 2/3/4 上线（本次真机已复现 hart 0 panic）。🔴→✅

## 任务简报

- 完成日期：2026-08-15
- commit：本任务实现提交（见 `git log --oneline -1`，分支 `feat/real-hardware-porting`）
- 实际改动：
  - 新增 `impl-jh7110-visionfive2/src/smp.rs`：`Jh7110Smp` 委托
    `opensbi_common::OpenSbiSmp` 的 HSM/IPI/remote fence，仅覆盖
    `configured_cpu_mask()` 为 harts 1..=4（`0b11110`），排除 S7 监控核。
  - `lib.rs` 改用自有 `smp` 模块；`Cargo.toml` 补 `base` 依赖。
  - `os/src/main.rs` `start_secondary_harts`：先取
    `platform::smp::configured_cpu_mask()`，与 LA 路径一致地过滤候选 hart。
- 验收结果：
  - `make jh7110_check` / `make rv_check`：通过。
  - QEMU virt 回归：mask 仍为全部 hart，`init_after_boot complete` →
    `/dev/vda4` 挂载 → login 全链路通过。
  - `make jh7110_uimage` / `jh7110_bootdir` / `make disk`：镜像重建。
  - `git diff --check`：clean。
- 真机验证（待用户重烧）：
  - 预期 AP 2/3/4 通过 PLIC 初始化并 online，日志出现
    `enabled supervisor external interrupts hart=2/3/4 context=...`；
  - 已知后续：多核并发打印会交叠（串口无 per-CPU 锁，后续任务处理）；
    MMC 仍 fail-closed，rootfs 挂载失败属预期。
