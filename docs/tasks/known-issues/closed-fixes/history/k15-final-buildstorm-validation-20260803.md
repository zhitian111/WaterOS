# Final BuildStorm 完整验证记录

## 验证范围

- 架构：RISC-V64/OpenSBI，8 个 vCPU，8 GiB 内存
- 内核：`kernel-rv-final-log`
- 根文件系统：主办方镜像的临时 qcow2 overlay
- 测试顺序：`cagent-glibc` 后执行 `buildstorm-glibc`
- 覆盖修复：futex COW 键、共享文件映射回写、ext4 索引目录线性化、
  多线程 `exec`/`clone` 屏障

## 运行结果

串口日志给出完整成功标记：

```text
BUILDSTORM_BEGIN mode=multi
Finished `release` profile [optimized] target(s) in 4m 08s
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=464.86 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

`cagent-glibc` 同样正常结束。运行期间诊断计数持续增长，最终构建未发生
死锁、panic 或超时。

## 文件系统检查

QEMU 正常退出后，对 overlay 执行：

```bash
qemu-img check /tmp/wateros-final-clean-buildstorm-20260803.qcow2
e2fsck -fn /dev/nbd0
debugfs -R 'stat /work/tgoskits/target/riscv64gc-unknown-linux-musl/release/arceos-helloworld' /dev/nbd0
```

结果如下：

- `qemu-img check`：`No errors were found on the image.`
- `e2fsck -fn`：五阶段检查完成，没有目录、引用计数或位图错误；仅提示两个
  extent tree 可以压缩，这是优化建议，不是损坏。
- 产物 inode 23815：大小 1,681,000 字节，`Blockcount: 3304`，具有完整 extent
  映射，排除了只更新文件长度但未回写数据的问题。
- 原索引目录 inode 22501 的 flags 为 `0x80000`，目录结构和 checksum 校验通过。

## 可复核材料

- 串口日志：`/tmp/wateros-final-clean-buildstorm-20260803.log`
- 日志 SHA-256：`05a6b5df8941eed2af02b8d13b023dc80fe42906753b3d47fc19c994c53eb771`
- 内核 SHA-256：`ba1df5155e530136c2552388bd8eb2266380ff6b8a552051fd6608050406ba4d`

结论：当前修复组合已通过最终镜像上的完整 8 核 BuildStorm，且测试后的
ext4 文件系统保持一致。该结果只覆盖上述测试组，不等同于全部决赛测试通过。
