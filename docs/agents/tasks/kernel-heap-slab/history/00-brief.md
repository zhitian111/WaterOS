# Task 00 简报：基线采集过程记录

## 状态

已完成。两架构各取得 3 轮成功样本并记录基线中位数。

## 基线构建

- 基线提交：`59f50c44`
- `make kernel-rv-final` SHA-256：
  `0ce76fba044687f3f59dba4dd0c833b9a7c302135500405b78b62148ef7d96bb`
- `make kernel-la-final` SHA-256：
  `a663db04ddb7bd0121df9682dcb9e0f292fbf2b9787ce294d05c000121d41b38`
- 保存路径：`/home/zhitian/project/WaterOS_refactor/os/.perf-baseline-main/`

## 外部输入审计

```text
2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2  sdcard-la-pub.img.gz
cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1  sdcard-rv-pub.img.gz
84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb  buildstorm_testcode.recovered.sh
```

## LoongArch64 baseline round 1

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-la.img`
- 脚本：`/glibc/buildstorm_testcode.sh`，size 7549，mode 0755
- QEMU：9.2.1，`-m 36G -smp 12 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=12 elapsed_s=556.91 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-la-baseline-1.log`
- 日志 SHA-256：`820b98e7f650e82bab80c7ac84d2c377d797080bf797c18fdd0c0ea7a3313a10`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## LoongArch64 baseline round 2

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-la.img`（round 2 前重新解压）
- QEMU：9.2.1，`-m 36G -smp 12 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=12 elapsed_s=585.73 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-la-baseline-2.log`
- 日志 SHA-256：`001b6aecf13ff92effa57ca293dd0df2c68ea1d0ebfeb864c172a623ef26a06e`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## LoongArch64 baseline round 3

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-la.img`（round 3 前重新解压）
- QEMU：9.2.1，`-m 36G -smp 12 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=12 elapsed_s=562.56 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-la-baseline-3.log`
- 日志 SHA-256：`a2b293817165c21f52e2eaf16f24d4cce2d06e456004734c1d542e05f9b133f6`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## LoongArch64 baseline 汇总

| round | elapsed_s |
|---|---|
| 1 | 556.91 |
| 2 | 585.73 |
| 3 | 562.56 |

- 中位数：`562.56`
- 三轮均 `status=OK rc=0 run=OK all commands finished`。

## RISC-V64 baseline round 1

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-rv.img`
- 脚本：`/glibc/buildstorm_testcode.sh`，size 7549，mode 0755
- QEMU：9.2.1，`-m 16G -smp 8 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=562.57 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-rv-baseline-1.log`
- 日志 SHA-256：`85ed2330d26e6402c660247cc3a28f3afcf2d658b03676f39428611f227dd2c3`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## RISC-V64 baseline round 2

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-rv.img`（round 2 前重新解压）
- QEMU：9.2.1，`-m 16G -smp 8 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=580.40 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-rv-baseline-2.log`
- 日志 SHA-256：`c906173b0c37561bc2d437f4e672b2610a79c3ba9fcf1c30c39c2a414958ccb7`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## RISC-V64 baseline round 7（成功样本 3/3）

- 临时镜像：`/home/zhitian/Downloads/wateros-slab-rv.img`（重新解压）
- 内核：`/home/zhitian/project/WaterOS_refactor/os/.perf-baseline-main/kernel-rv-final`
- QEMU：9.2.1，`-m 16G -smp 8 -snapshot`
- 结果：

```text
TOOLCHAIN_RESULT status=OK
MINIBUILD_RESULT status=OK
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8 elapsed_s=553.72 ... run=OK
all commands finished
```

- 日志：`/tmp/wateros-slab-rv-baseline-7.log`
- 日志 SHA-256：`69beb5cf6de2a5ea989c113cc6b6c88f19e2185566c508561f2ad98946c1f239`
- 关键标记检索未发现 `[heap] OOM`、`ENOMEM`、`SIGSEGV`、kernel panic。

## RISC-V64 baseline 失败/无效样本记录

- round 3/4/5：误用被 Task 01 重建覆盖的 worktree 内核，**不作为基线样本**；
- round 6：使用保留的 baseline 内核，仍复现 guest `SIGSEGV`，`status=FAIL`，
  日志 `/tmp/wateros-slab-rv-baseline-6.log`；
- 结论：当前 main 在 RV 上存在既有的间歇性 guest SIGSEGV，与 Task 01 改动无关。

## RISC-V64 baseline 汇总（有效样本）

| round | elapsed_s |
|---|---|
1 | 562.57 |
2 | 580.40 |
7 | 553.72 |

- 中位数：`562.57`
- 有效三轮均 `status=OK rc=0 run=OK all commands finished`。

## 最终基线汇总

| 架构 | elapsed_s 样本 | 中位数 |
|---|---|---|
| LA | 556.91 / 585.73 / 562.56 | 562.56 |
| RV | 562.57 / 580.40 / 553.72 | 562.57 |

最终性能验收目标：最终分支两架构中位数均小于上述 baseline 中位数。
