# LoongArch BuildStorm 嵌套 QEMU 缺少 UAL HWCAP

## 现象

LoongArch BuildStorm 完成 `arceos-helloworld` ELF/BIN 构建后，线上附加运行验证没有
得到 helloworld 输出，而是终止于：

```text
qemu-system-loongarch64: TCG: unaligned access support required; exiting
```

这使包装脚本无法输出最终的成功结果，整项没有分数。

## 实际测试链路

- 仓库和官方 `testsuits-for-oskernel` `final-2026` 最新公开脚本只执行
  `cargo xtask arceos build`，judge 解析最后一条 `BUILDSTORM_COMPILE`。
- 线上平台另有 build 后运行验证；日志名为 `buildstorm.run.out`。
- tgoskits 的 LoongArch helloworld 配置使用 `qemu-system-loongarch64 -machine virt
  -cpu la464 ...` 启动生成的 BIN，成功条件为串口出现 `Hello, world!`。
- 因此该 QEMU 是运行在 WaterOS LoongArch 用户态里的嵌套 TCG 进程，不是开发机上
  启动 WaterOS 的外层 QEMU。

## 版本核对

本地 `os/sdcard-la-pub.img` SHA-256 为
`bf4ffe125052e3d5608c0705e7364c4bf87dc5dc1f7c48365425f0886a9887d0`。从镜像直接
提取的 `/opt/qemu-la64/bin/qemu-system-loongarch64`：

- 自带版本字符串：QEMU 10.1.0（Debian `1:10.1.0+ds-1`）；
- 二进制 SHA-256：
  `7d74379dc8366ae8ce541bb91ed7a22d9b257a092ac3b205eb73591806c08d47`；
- 包含上述 TCG 退出字符串。

线上测试容器由平台确认使用 QEMU 9.2.1。上游 QEMU 9.2.1 与 10.1.0 的
`tcg/loongarch64/tcg-target.c.inc::tcg_target_init` 条件相同：读取
`getauxval(AT_HWCAP)`，缺少 `HWCAP_LOONGARCH_UAL = 1 << 2` 就在加载被测 guest 前退出。

## 根因与修复

WaterOS 的 LoongArch ELF auxv 只宣告 `HWCAP_LOONGARCH_FPU = 1 << 3`。项目目标 QEMU
`la464` 的 CPUCFG1.UAL 为 1，硬件模型支持非对齐访问，但 WaterOS 没有把该能力发布给
用户态。

修复在共用 ELF 用户栈构造处同时发布 UAL 与 FPU；不发布 LSX/LASX 等尚未保存恢复的
扩展。定向测试固定 bit 2/bit 3，并确保没有意外增加其他位。

## 验证门槛

1. MM API 定向测试通过；RISC-V 与 LoongArch check/build 通过。
2. LoongArch 短启动中，镜像内 QEMU 不再输出 UAL 缺失错误。
3. 使用预构建 `arceos-helloworld.bin` 跑完整嵌套 QEMU 路径，串口出现
   `Hello, world!`。仅验证 `qemu-system-loongarch64 --version` 不足以关闭问题。

## 已完成验证

- `wateros-mm-api-v0` host tests：通过。
- RISC-V/LoongArch Final check：通过。
- 修复后 LoongArch Final build：通过，内核 SHA-256 为
  `ebe1b2a8fb84c6e3a31ad61697a3f3e3f6a4d554581d0ad16506cfaa6ac99692`。
- 使用同一 LA 镜像的 5 秒 TCG 初始化探针做 matched 对照：
  - 修复前内核
    `1c87d964eb9c21fe6e75e6da2e338a2cc961a5bd0ba54c181e23962ec79f550c`：
    QEMU 10.1.0 输出 UAL 缺失并以 `rc=1` 退出；
  - 修复后内核：QEMU 越过 TCG 初始化，保持运行直到探针 timeout，输出
    `BUILDSTORM_UAL_TCG_INIT ok`；
  - 两轮均使用同一临时 qcow2 overlay，原始 14 GiB 镜像未被探针修改。

完整 `Hello, world!` 验证将追加到 RV/LA 性能镜像的 BuildStorm 脚本：正式编译计时结束后
复用刚生成的产物执行 `cargo xtask arceos qemu`，成功后再发布最终 compile 结果。这样每次
性能测试同时覆盖线上额外运行步骤，不另做一次长编译。

## 性能镜像脚本接线

已直接更新两个本地性能镜像的 `/glibc/buildstorm_testcode.sh`，没有复制或备份大镜像：

| 架构 | 修改前镜像 SHA-256 | 修改后镜像 SHA-256 |
| --- | --- | --- |
| RISC-V | `4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1` | `88e22cd5d3ba89aacecc1e16b77f1e38cbf952246902b280df4d87b88ee9ff78` |
| LoongArch | `bf4ffe125052e3d5608c0705e7364c4bf87dc5dc1f7c48365425f0886a9887d0` | `7957779256dcc21507ca32f9b0e78c0a8ebc09bc2e142568d64fe44241963775` |

原脚本只备份为镜像旁的小文件：

- `os/sdcard-rv-pub.img.buildstorm_testcode.sh.bak-20260811`；
- `os/sdcard-la-pub.img.buildstorm_testcode.sh.bak-20260811`。

两份原脚本 SHA-256 均为
`5bfbaa5bd99bec595ccd980fff0ebc002d42c0ce0a7e28af266f0ddad69b7189`。更新后两镜像内脚本
SHA-256 均为 `914b38966173e11cc282b1dab073b63b2a714c0a239518b941ced3052e24b99d`，权限为
`0755`，并通过 `sh -n`。

脚本先记录 `T1` 和 `ELAPSED`，再执行最多 300 秒的 untimed
`cargo xtask arceos qemu -p arceos-helloworld --arch "$AXARCH"`。只有命令返回 0 且
`buildstorm.run.out` 包含 `Hello, world!`，才输出 `BUILDSTORM_RUN ok` 和最终
`BUILDSTORM_COMPILE ... ok=true`。本地 `/opt/qemu-{rv,la}64/lib` 仅在 run 阶段加入
`LD_LIBRARY_PATH`，不会改变正式 Rust 编译的动态库环境或计时。
