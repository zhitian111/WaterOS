# BS-SC-00：建立 BuildStorm 双架构验收工具

## 任务内容

在当前分支落地可复现的镜像准备器和 BuildStorm runner。该提交只改 host 工具、测试和
工具文档，不修改内核语义。后续所有性能结论必须由本工具产生结构化结果。

## 实施方案

1. 新增 `os/scripts/perf/prepare_buildstorm_image.py`：
   - 校验三份固定输入 SHA-256；
   - 通过流式 gzip 解压到 `os/tem/perf/buildstorm-singlecore/images/`；
   - 检查 raw 镜像恰为 15,032,385,536 字节；
   - 使用 `debugfs` 在镜像副本中替换 `/glibc/buildstorm_testcode.sh`；
   - 设置普通文件和 `0755` 权限，dump 回宿主后重新核对脚本 SHA-256；
   - 运行 `e2fsck -fn`，只接受无文件系统错误的母盘；
   - 不修改、删除或重压 `/home/zhitian/Downloads` 中的输入。
2. 新增 `os/scripts/perf/buildstorm_runner.py`：
   - 每个 run-id 独占输出目录，禁止覆盖；
   - 每轮从母盘创建 reflink/稀疏运行副本，不使用 `-snapshot`；
   - 强制使用 `/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-*`；
   - 启动前验证 QEMU 首行版本为 9.2.1；
   - QEMU argv 与任务总 README 中的架构、内存、SMP 和设备契约完全一致；
   - 解析 `TOOLCHAIN_RESULT`、`MINIBUILD_RESULT`、`BUILDSTORM_RESULT`；
   - 记录 git SHA、内核/母盘/运行脚本哈希、完整 argv、guest elapsed、host wall、
     panic/SIGSEGV/timeout 和串口日志到 `result.json`；
   - plugin 诊断结果明确标记为不可用于墙钟验收。
3. 新增单元测试，覆盖 argv、版本拒绝、marker 最后一次结果、超时清理、run-id 不覆盖、
   `<500/500..520/>=520` 判定和 dry-run。
4. 同步 `os/scripts/README.md` 与 `docs/tools/` 中的性能运行说明。

不要原样复制 `perf/tlsf-slab` 的旧 runner：它解析旧的 `BUILDSTORM_COMPILE` 协议且默认
追加 `-snapshot`，与本任务契约不符。

## 涉及文件

- `os/scripts/perf/prepare_buildstorm_image.py`（新增）
- `os/scripts/perf/buildstorm_runner.py`（新增）
- `os/scripts/perf/README.md`（新增）
- `os/scripts/tests/test_buildstorm_runner.py`（新增）
- `os/scripts/tests/test_prepare_buildstorm_image.py`（新增）
- `os/scripts/README.md`
- `docs/tools/README.md` 及新增/现有 BuildStorm 工具页
- `history/00-brief.md`（完成时新增）

## CodeGraph 查询

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore
codegraph explore "buildstorm_testcode.sh final_online run_stage_busybox"
codegraph explore "QemuLaunch build_qemu_launch qemu run scripts"
codegraph impact "run_stage_busybox"
```

## 验收命令

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
python3 scripts/tests/test_buildstorm_runner.py
python3 scripts/tests/test_prepare_buildstorm_image.py
python3 scripts/perf/prepare_buildstorm_image.py --dry-run --arch rv
python3 scripts/perf/prepare_buildstorm_image.py --dry-run --arch la
python3 scripts/perf/buildstorm_runner.py --dry-run --arch rv \
  --kernel tem/perf/buildstorm-singlecore/baseline-main-0c4eadf2/kernel-rv-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-rv-pub-prepared.img \
  --run-id harness-rv-dry --timeout 1800
python3 scripts/perf/buildstorm_runner.py --dry-run --arch la \
  --kernel tem/perf/buildstorm-singlecore/baseline-main-0c4eadf2/kernel-la-final \
  --image tem/perf/buildstorm-singlecore/images/sdcard-la-pub-prepared.img \
  --run-id harness-la-dry --timeout 1800
git diff --check
```

实际解压/覆写不要求在单元测试中重复两次，但本任务完成前必须至少真实准备两架构母盘，
执行 dump-back 哈希和 `e2fsck -fn`。本任务不运行 main 性能 baseline。

## 完成后简报

新增 `history/00-brief.md`，记录工具测试结果、两份母盘路径/大小/哈希、dump-back 脚本哈希、
e2fsck 结论、实际 QEMU 9.2.1 版本和 baseline ELF 清单。
