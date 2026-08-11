# 2026-08-08：`copy_to_user_progress` 写就绪地址合并实验（已回退）

## 思路

写路径原实现每页执行 `leaf_page_perm`、COW/private 检查、`translate_addr` 多次页表
walk。尝试新增 `prepare_user_write_addr()`，一次返回 COW/private 后的物理地址，减少
重复翻译。

## 结果

RISC-V Final 启动后 CAgent 脚本立即失败：

```text
program=/glibc/cagent_testcode.sh command failed exit_code=1; stop queue
```

pc-hot 采集约 1.7s 即退出，无法形成有效 A/B。该改动同时涉及 RISC-V 与 LoongArch 的
页表和 user_copy，属于高风险未验收修改，已完整回退。

## 结论

- 不能在没有完整调试前保留该方向。
- 下一步需要先用最小 COW/write 用户程序定位是权限检查、COW 后 PA 刷新还是 LA/RV
  差异导致失败。
- 在此之前，`copy_to_user_progress` 保持已提交的稳定实现。
