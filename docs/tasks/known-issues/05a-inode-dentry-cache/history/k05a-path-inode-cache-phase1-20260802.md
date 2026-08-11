# K-05A path/inode cache 第一阶段报告（2026-08-02）

## 记录信息

- base_commit: `8a06614d94f5aff6ae04e508365914c8abb5042e`
- user_submodule: `2f470f95fa6bf0401c4b1b7ef3bb8fc7a10b870b`
- QEMU: 11.0.2，RISC-V64/OpenSBI，8 CPU
- pre image: `sdcard-rv.img`，SHA-256
  `7deebc7a558e9d24567d13bc54c581913a5ff05d5ae5788097e02756a0424c15`
- final image: `sdcard-rv-pub.img`，SHA-256
  `dd9bbc442f990b228087f15c8da14776981eb38ee393a84a89daf39e46c119d0`
- 所有运行均使用全新 qcow2 overlay，未写原镜像。

## 现象与定位

final 镜像中相同工具重复运行没有热缓存收益。基线每次 `rustc --version` 产生约
6773 次用户缺页并耗时 10.86–11.51 秒，`cargo --version` 耗时 3.98–4.10 秒。
ELF lazy fault 每页调用 another-ext4 `read_range()`；旧实现每次都递归解析完整路径，
随后额外 `getattr`，再按 inode 读取实际数据。

两个实验被数据否定并已撤销：让 ELF loader 经过同步 page cache 使 `rustc` 退化到
约 12 秒；每次 fault 预装 4 页虽将 fault 降低约 64%，却使 `rustc` 退化到约 17 秒。
这证明瓶颈是每个实际读取页的路径/元数据成本，而不是单纯 fault 次数。

## 修改

- 在 another-ext4 适配层增加容量 4096 的有界 path→inode cache；满时整体回收，
  防止长跑中路径字符串无界占用内核堆。
- mount 清空缓存；create/mkdir/mknod/hardlink 插入对象；unlink/rmdir 删除路径子树；
  rename 原子迁移源子树并清除目标子树。
- `exists`、`metadata`、`read_range`、`read_dir`、`read_symlink` 复用 inode。
- `read_range` 直接使用 vendor `Ext4::read` 的文件类型和 EOF 检查，删除重复 `getattr`。
- 未修改 task 模块、调度器、VFS API 或 vendor another_ext4。

## 验证与结果

- another-ext4 host tests：2 passed，覆盖 rename/remove 子树失效。
- `make rv_check`、`make la_check` 通过。
- final 8 核微基准：
  - `rustup --version`：10.20–11.20 秒降至 3.16–3.60 秒；
  - `rustc --version`：10.86–11.51 秒降至 3.58–3.60 秒；
  - `cargo --version`：3.98–4.10 秒降至 1.36–1.41 秒；
  - 用户 fault 数基本不变，无 panic、SIGSEGV、BadFd、OOM 或停滞。
- pre 8 核根 ext4 回归通过：重复读取、文件/目录 rename、hardlink、unlink、同名重建
  均获得预期内容，退出 0；overlay 的 `e2fsck -fn` 五阶段通过。

原始日志及 SHA-256：

- `/tmp/wateros-toolchain-baseline.log`：
  `7dbdba726b751c67b7ee032df7f6ccfaf9cc5ac617b4d7c7a9f72eaef32b66db`
- `/tmp/wateros-toolchain-inode-cache.log`：
  `42d6de6e38ccabeea93cff70243030aa0b64ea120cea3cab0f63b195f511b13a`
- `/tmp/wateros-inode-cache-pre2.log`：
  `45b5ef157fec5dc0fa9d0c59bbfe68830a734de5f1cc3931375eef528db77b4b`
- `/tmp/wateros-inode-cache-pre2-e2fsck.log`：
  `1ee40b42b8894134c6302d51a5ca372747ece50365ec0664b1515eee4c034841`

## 未关闭门禁

3 分钟 BuildStorm-only 探针快速完成 toolchain，但在静默 minibuild `cargo build` 中未
创建 `target/`；日志无 fault/panic，不能视为 BuildStorm 通过。稳定 open-file object
handle 也仍属于 K-05A 后续阶段。下一步应给 minibuild 增加阶段性诊断，区分 Cargo
registry/index 扫描、锁等待、metadata/stat 和进程等待，再做对应优化。完整 CAgent、
BuildStorm 和 final overlay `e2fsck` 留到夜间门禁。
