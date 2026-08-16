# Task 00 简报：main 基线与测试台固化

## 完成情况

已在主仓库 `main`（commit `0c4eadf2`）构建默认 TLSF Final 内核，并复制到本工作树的
`os/.perf-baseline-slab-singlecore-main/`。该目录是本地性能基线产物，不提交到 git。

## 基线 SHA-256

```text
2f85662abc3ab6987f69066ec282bd7c11af5eaccaa122ad00b583db4ddbb80d  kernel-rv-final
d4a3a7bba55d2c026f28a58f7a4e3fcfdfc42737aace18936349e14126196547  kernel-la-final
cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1  ~/Downloads/sdcard-rv-pub.img.gz
2c411447274fbd83505d2fac505a5d9e8edff3bdfc3d2d6cbdb8f61ff7d90d2  ~/Downloads/sdcard-la-pub.img.gz
84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb  ~/Downloads/buildstorm_testcode.recovered.sh
```

QEMU 两架构均为 `9.2.1`，路径为 `~/qemu_9_2_1/qemu-9.2.1/build`。

## 验证命令

```text
make kernel-rv-final                 PASS
make kernel-la-final                 PASS
QEMU --version                       PASS (9.2.1)
```

尚未运行完整 BuildStorm 基线轮；这是后续性能验收任务的首项运行工作。原始 gzip 镜像未被修改。

## 2026-08-17 当前 main 重基线

主线更新后，专用分支已合并 `main` commit
`a6cf25158116a9f182c49c31b8e2ad8938faa415`。为避免 RISC-V `Info` 热路径串口日志污染
结果，基线只增加 `qemu-riscv64-opensbi` 从 `runtime/impl-info` 到
`runtime/impl-error` 的编译期配置变化；信号修复代码未改动。

```text
15ce972b339005cf23bd7fb42e9c072b39257418ddb552f7762798dc7ab1bddc  kernel-rv-final
cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1  ~/Downloads/sdcard-rv-pub.img.gz
84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb  ~/Downloads/buildstorm_testcode.recovered.sh
```

QEMU 使用 `~/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64` 9.2.1，参数为
16 GiB、8 vCPU、`-snapshot`。镜像从原 gzip 重新解压，覆写脚本后 guest 内脚本 SHA-256
与源文件一致，mode 为 `0755`。

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=549.68
artifact=target/riscv64gc-unknown-linux-musl/release/arceos-helloworld bytes=1681000 run=OK
完整脚本 elapsed=584.823s，all commands finished
```

这是按用户要求保留的单轮 RISC-V baseline，不计算中位数。日志位于
`/tmp/wateros-main-a6cf2515-errorlog-rv-buildstorm.log`；本地基线内核位于
`os/.perf-baseline-slab-singlecore-main-a6cf2515-errorlog/kernel-rv-final`。实验 raw 镜像已删除，
原始 gzip 未修改。今后性能运行只做 RISC-V；两架构静态、构建和功能正确性仍为硬门禁。
