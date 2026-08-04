# RIO-10 双架构决赛工作负载门禁

## 范围

在提交 `196975b8` 上使用主办方决赛镜像，分别以 8 CPU、8 GiB 内存运行 CAgent 与
BuildStorm。两次 QEMU 均启用 snapshot，未写回基础 raw 镜像。内核启动命令保持正式
`final_online` 队列，不加入测试专用 syscall 或 helper。

## 命令

```bash
cd os
make kernel-rv-final
WOS_QEMU_SNAPSHOT=1 WOS_SMP=8 bash scripts/rv_final_run.sh

make kernel-la-final
WOS_QEMU_SNAPSHOT=1 WOS_SMP=8 bash scripts/la_final_run.sh
```

## 结果

| 架构 | CAgent | BuildStorm | 编译耗时 | 产物字节 | 退出 |
|---|---:|---|---:|---:|---|
| RISC-V64 | 10/10 | `ok=true` | 1464.91s | 1,681,000 | 正常 |
| LoongArch64 | 10/10 | `ok=true` | 1101.46s | 1,714,568 | 正常 |

两边均依次输出 `BUILDSTORM_TOOLCHAIN ok`、`BUILDSTORM_MINIBUILD ok` 和
`BUILDSTORM_COMPILE mode=multi ok=true`。主命令结束后 bringup runner 输出
`command succeeded` 与 `all commands finished`，没有复现此前偶发的产物生成后退出
卡住。LoongArch Cargo 输出一次 last-use 缓存时间转换警告，但继续完成全部编译，
不影响结果。

## 证据

```text
/tmp/wateros-final-rv-196975b8-20260804.log
SHA-256 e1030a795228c83d8ee3f3058f423d55591408c30c5c5124e678f5b1305b4588

/tmp/wateros-final-la-196975b8-20260804.log
SHA-256 8e62371df09aa4bf9c256abbb406e32d0500edfd6641d0f615af1da3e889c4c8
```

测试后基础镜像哈希不变：

```text
RISC-V64  83073eb1c5b85def0aba3031300a7c7c3f4594c7a68bfa146ae01d4a076a6abb
LoongArch cf8660bdc216d3dd6c82f4b50cdc4271d1be6dc49eb647ccbb9a0f24f36ad245
```

## 剩余限制

本报告关闭双架构决赛 workload 门禁，不单独宣称 RIO-10 全部完成。初赛镜像中由既有
不适配清单删除的 12 个 LTP 二进制仍需等价语义覆盖；Linux 差分、长时间 SMP 内存竞态
和性能/资源基线也仍按 `10-integration-and-regression.md` 执行。
