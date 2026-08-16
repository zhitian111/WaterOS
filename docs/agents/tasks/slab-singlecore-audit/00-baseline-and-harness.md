# Task 00：固化 main 基线与性能测试台

## 任务目标

保存当前 `main` 的 RV/LA Final 内核，验证外部镜像和恢复脚本输入，固化 QEMU 9.2.1
启动命令、日志标记和统计口径。该提交只包含任务文档/必要测试工具，不修改 allocator。

## 实施方案

1. 在主仓库的 `main` 工作树构建 `kernel-rv-final` 和 `kernel-la-final`，复制到任务专用
   基线目录，例如 `os/.perf-baseline-slab-singlecore-main/`，记录 SHA-256、git commit、
   rustc、QEMU 版本。
2. 校验：
   `sha256sum ~/Downloads/sdcard-{rv,la}-pub.img.gz ~/Downloads/buildstorm_testcode.recovered.sh`。
3. 每次测试分别执行：

   ```bash
   gzip -dc ~/Downloads/sdcard-la-pub.img.gz > /tmp/wosaudit-la-<run>.img
   debugfs -w -R 'rm /glibc/buildstorm_testcode.sh' /tmp/wosaudit-la-<run>.img
   debugfs -w -R 'write /home/zhitian/Downloads/buildstorm_testcode.recovered.sh /glibc/buildstorm_testcode.sh' /tmp/wosaudit-la-<run>.img
   debugfs -w -R 'set_inode_field /glibc/buildstorm_testcode.sh mode 0100755' /tmp/wosaudit-la-<run>.img
   debugfs -R 'stat /glibc/buildstorm_testcode.sh' /tmp/wosaudit-la-<run>.img
   ```

   RV 使用对应的 `sdcard-rv-pub.img.gz`。覆盖后用 `debugfs -R cat` 和 `sha256sum` 验证脚本
   内容，不修改 `~/Downloads` 下的原始文件。
4. 启动前检查无遗留 QEMU/runner；所有命令使用 `-snapshot`，保存完整串口日志和结果摘要。

## 验收方式

### 静态/构建

```bash
cd /home/zhitian/project/WaterOS_refactor/os
make kernel-rv-final
make kernel-la-final
sha256sum kernel-rv-final kernel-la-final
```

### QEMU 命令

LA：

```bash
~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-loongarch64 \
  -kernel kernel-la -m 36G -nographic -smp 12 \
  -drive file=/tmp/wosaudit-la-<run>.img,if=none,format=raw,id=x0 \
  -device virtio-blk-pci,drive=x0 \
  -no-reboot -device virtio-net-pci,netdev=net0 -netdev user,id=net0 -rtc base=utc \
  -snapshot
```

RV：

```bash
~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64 \
  -machine virt -kernel kernel-rv -m 16G -nographic -smp 8 -bios default \
  -drive file=/tmp/wosaudit-rv-<run>.img,if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
  -no-reboot -device virtio-net-device,netdev=net -netdev user,id=net -rtc base=utc \
  -snapshot
```

每个架构至少完成一轮 main smoke，日志必须包含 `TOOLCHAIN_RESULT status=OK`、
`MINIBUILD_RESULT status=OK`、`BUILDSTORM_RESULT ... status=OK ... run=OK` 和
`all commands finished`，且无 panic/OOM/ENOMEM/SIGSEGV/fault。

## 涉及文件与 CodeGraph

- `os/Makefile`、`os/scripts/`、`os/src/user_bringup_common.rs`
- `docs/agents/tasks/slab-singlecore-audit/`

```bash
codegraph explore "user_bringup_common buildstorm command completion markers"
codegraph callers "activate_slab register_frame_source"
```

## 完成后

新增 `history/00-brief.md`，记录基线内核 SHA、镜像 SHA、QEMU 版本、每轮耗时和环境限制。

