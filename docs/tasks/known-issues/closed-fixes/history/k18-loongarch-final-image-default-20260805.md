# K-18 LoongArch Final 默认镜像修复（2026-08-05）

## 问题

`la_final_run.sh` 默认使用初赛入口的 `sdcard-la.img`，而当前 LoongArch 决赛镜像为
`sdcard-la-pub.img`。直接执行 `make la_final_run` 会在前者不存在时由 QEMU 立即
退出；RISC-V final 已经默认使用对应的 `sdcard-rv-pub.img`。

## 修改

仅将 LoongArch final 启动脚本的默认磁盘改为 `./sdcard-la-pub.img`。显式传入
`WOS_SDCARD` 的行为保持不变，`la_pre_run.sh` 仍使用初赛镜像。

## 验证

```text
bash -n os/scripts/la_final_run.sh
default:  -drive file=./sdcard-la-pub.img,...
override: -drive file=./custom-la.img,...
```

在修改前，使用显式 `WOS_SDCARD=./sdcard-la-pub.img` 的 8 核 LoongArch final
验证已完成 CAgent 10/10 和 BuildStorm；结果见
`k22-dual-arch-final-buildstorm-20260805.md`。
