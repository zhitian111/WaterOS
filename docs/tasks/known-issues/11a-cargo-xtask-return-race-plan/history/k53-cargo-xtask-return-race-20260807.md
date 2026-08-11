# K-53 `cargo xtask` 返回竞态修复报告（2026-08-07）

## 问题

BuildStorm 的 `cargo xtask arceos build` 在 `[axbuild] ... done` 后偶发不返回，
导致 shell 管道无法读取 `/work/.build.rc`，最终不打印 `BUILDSTORM_COMPILE`。
该问题同时出现在 RISC-V 和 LoongArch 完整 Final 中。

## 根因

`exit_group` 已发布 `ProcessState::Exiting`，但旧实现只在 trap 进入且即将返回用户态时
检查一次。sibling 线程若已阻塞在内核 syscall，被唤醒后直接回到用户态，可能错过该
检查，导致进程不能全部退出，`cargo xtask` 的父 shell 一直等待。

## 修复

`os/src/trap_handler.rs`：

1. 新增 `exit_current_if_process_exiting()`，在 trap 返回用户态前再次检查
   `ProcessState::Exiting`。
2. 在 `finish_trap_return()` 和统一返回路径调用该检查，覆盖阻塞 syscall 唤醒、
   IPI/timer 和普通 syscall 返回路径。
3. timer 中断进入内核态且当前进程已是 `Exiting` 时，也调用
   `exit_group_current`，覆盖长内核路径中不主动返回用户态的 sibling。

## 验证

### 构建

```text
make rv_check 通过
make la_check 通过
```

### RISC-V Final

| 轮次 | 结果 |
|---|---|
| 第一次 | `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1325.92` |
| 第二次（补强后） | `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1296.63` |

日志：

```text
/tmp/k53-full-rv.log
/tmp/k53-full-rv2.log
```

### LoongArch Final

| 轮次 | 结果 |
|---|---|
| 补强前 | axbuild `done` 后约 7 分钟未返回，命中原竞态 |
| 补强后第一次 | `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1220.33` |
| 补强后第二次 | `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1404.17` |

日志：

```text
/tmp/k53-full-la.log
/tmp/k53-full-la2.log
/tmp/k53-full-la3.log
```

### RISC-V Pre smoke

120 秒窗口内完成 cyclictest/hackbench 并进入 LTP，无 panic、无 fatal kernel trap：

```text
/tmp/k53-pre-rv.log
```

## 结论

K-53 已闭环。修复后的 RISC-V 两轮和 LoongArch 两轮均稳定输出
`BUILDSTORM_COMPILE ok=true`，不再依赖“重跑一次碰运气”。
