# Task 07 LA12 资源阻塞记录

## 状态

截至当前，VMA 统一路径已通过以下验证：

| 架构 | 配置 | BuildStorm 结果 | elapsed_s | 日志 |
|:--|:--|:--|:--|:--|
| RV | 单核 | `status=OK run=OK` | 1329.63 | `/tmp/wateros-vma-rv-single.log` |
| RV | 8 核 | `status=OK run=OK` | 547.65 | `/tmp/wateros-vma-rv-smp8-clean.log` |
| LA | 单核 | `status=OK run=OK` | 1246.33 | `/tmp/wateros-vma-la-single.log` |
| LA | 12 核（功能验证，24G） | `status=OK run=OK` | 524.53 | `/tmp/wateros-vma-la-smp12-func24g.log` |
| LA | 12 核（最终性能，36G） | 未完成 | - | - |

静态验收已通过：

```text
make rv_check          PASS
make la_check          PASS
make kernel-rv-final   PASS
make kernel-la-final   PASS
git diff --check       PASS
```

## LA 12 阻塞原因

LA 12 性能命令要求：

```bash
qemu-system-loongarch64 ... -m 36G ... -smp 12 ...
```

但当前宿主机：

```text
Mem:  30Gi total, 17Gi used, 3.7Gi free, 18Gi buff/cache
Swap: 8.0Gi total, 7.9Gi used
```

Firefox、WPS、QQ、GNOME Shell 等桌面进程持续占用大量内存。外层 QEMU
申请 36G 后，guest 在 BuildStorm 末尾需要再启动一个 LoongArch 嵌套 QEMU；
此阶段发生严重 swap/换页，表现是 cargo 构建已完成，但脚本停在
`boot arceos-helloworld in qemu` 之前，长时间不产生
`BUILDSTORM_RESULT`。

这不是已复现的内核逻辑错误，而是宿主资源不足导致的测试环境阻塞。

## 解除条件

- 关闭 Firefox、WPS、QQ 等大内存桌面应用，或迁移到内存更大的机器；
- 确认没有任何 `qemu-system-*` 正在运行；
- 重新从干净的 `sdcard-la-pub.img.gz` 生成测试镜像并替换
  `buildstorm_testcode.recovered.sh`；
- 使用 `/tmp/run_qemu_clean.sh` 运行 LA 12 命令，记录日志与 SHA-256。

## 下一步

已用 24G 完成 LA 12 核功能验证，说明 VMA 统一路径在 12 核下没有
新的功能性问题；仍需在宿主机内存充足后执行用户给定的 36G 性能命令。

最终 36G 通过后，再完成 `history/07-brief.md`，并更新 slab 分支的
`RECOVERY-REBASE.md` 交接信息。
