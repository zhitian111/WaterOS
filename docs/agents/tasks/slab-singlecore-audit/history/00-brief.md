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

