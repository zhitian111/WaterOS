# cred syscall

本目录把 Linux uid/gid、附加组和 capability 查询委托给 `wateros-cred`。

## 当前能力

- `get/set uid/gid`、`setre*`、`setres*`、`getres*`、`getgroups/setgroups`。
- `capget/capset` 的基础 Linux capability ABI 与 root 兼容策略。
- fork/clone/exec 生命周期由 task handler 调用 cred 侧表接口保持继承。

## 边界与扩展

当前没有 user namespace，部分特权检查用 euid 0 近似 capability。后续应加入
permitted/effective/inheritable/ambient 集、securebits、文件 capability 和
user-namespace 映射，并把 mount/reboot/chroot 等检查改为具体 capability。
