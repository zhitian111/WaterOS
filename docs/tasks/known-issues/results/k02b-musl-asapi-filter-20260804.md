# K-02B musl `asapi_01` 过滤报告（2026-08-04）

## 现象与定位

RISC-V64 和 LoongArch64 的 musl LTP `asapi_01` 均有 9 个 IPv6 协议项通过，但
`getprotobyname("hopopt")` 失败。启动期生成的 `/etc/protocols` 已包含
`hopopt 0 HOPOPT`，因此不是 rootfs 文件缺失。

LTP 20240524 明确要求 `hopopt` 返回协议号 0。musl 上游 `src/network/proto.c` 的
`getprotobyname` 不读取 `/etc/protocols`，只搜索编译期内建表；编号 0 的内建项为
`ip`，没有 `hopopt`。这属于测试与 musl libc 的兼容缺口，内核 syscall、VFS 或
root-layout 均无法改变已链接 libc 的查表结果。

对照资料：

- <https://raw.githubusercontent.com/linux-test-project/ltp/20240524/testcases/network/lib6/asapi_01.c>
- <https://git.musl-libc.org/cgit/musl/tree/src/network/proto.c>

## 修改

在 `os/src/user_bringup_ltp_exclusions.rs` 增加 musl 专属排除表，首项为
`asapi_01`。`os/src/user_bringup_root_layout.rs` 先按原公共清单过滤两套 libc，再只对
`/musl/ltp/testcases/bin` 应用 musl 清单。

没有将 `asapi_01` 加入公共表：glibc 会读取 `/etc/protocols`，实测 `hopopt` 及其余
9 个协议数据库断言全部通过；不支持 raw IPv6 socket 的后续检查以 `TCONF` 结束，
整个 glibc 用例退出 0。通用 syscall 行为未按测试程序名分支。

## 验证

- `make rv_check`、`make la_check` 和两架构 LTP-musl 内核构建通过。
- 修改前，两架构 musl 均为 9 TPASS、1 TFAIL、后续 2 TCONF，退出 33。
- LoongArch glibc 对照为 10 TPASS、2 TCONF，退出 0。
- 修改后，两架构 8 核启动均输出 `MUSL_ASAPI_PRUNED`、
  `GLIBC_ASAPI_PRESENT`，runner 正常结束。
- 公共清单保持 2352 项，新增 musl 专属清单 1 项。

过滤验证日志 SHA-256：LoongArch `d427f356...15c86`，RISC-V
`4de43b51...c366d`；glibc 对照为 `74d96c4c...93138`。
