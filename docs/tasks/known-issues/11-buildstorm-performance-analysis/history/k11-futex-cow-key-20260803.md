# BuildStorm futex COW 键值修复记录

## 问题

BuildStorm 并行编译中，父线程等待的 futex 位于私有/COW 映射。等待时系统把未带
`FUTEX_PRIVATE_FLAG` 的地址直接按物理地址建键；子线程退出写入 `clear_child_tid`
触发 COW 后，唤醒操作得到另一个物理地址，因而无法命中原等待队列。

## 修复

- MM 层区分真正跨地址空间共享的 VMA 与私有/COW VMA。
- 真正共享映射继续按物理字地址建键；私有/COW 映射按地址空间和用户 VA 建键。
- `clear_child_tid` 同时兼容 private 与 non-private futex，并避免相同键重复唤醒。
- RISC-V Sv39 与 LoongArch64 实现保持一致。

## 验收

- `make check`：通过。
- `make la_check`：通过。
- 8 核 BuildStorm 缓存镜像回归：原卡死线程成功被唤醒，日志中
  `clear_child_tid ... woken=1`，编译继续并在约 2 分 22 秒完成。
- 本次运行随后在读取链接产物时报告 `Unknown file magic`。离线检查确认产物只有
  文件长度、没有 extent，属于可写 `MAP_SHARED` 文件映射未回写的独立问题。

回归日志保存在 `/tmp/wateros-futex-cow-fix-probe-20260803.log`，SHA-256 为
`9a4975716ea17fe7bf1ec65902e4a72f3889ac87451293c9602cb6fae1a790ec`。
