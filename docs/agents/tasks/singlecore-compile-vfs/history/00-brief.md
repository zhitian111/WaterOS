# Task 00 简报：基线固化与缓存失效不变量

## 完成情况

本分支基线确定为 `perf/frame-allocator-pcpu`，并保存其双架构最终内核。

## 基线内核

```text
/home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore/kernel-rv-final
/home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore/kernel-la-final
```

SHA-256：

```text
82854e1aec97dbb895fd3ebec454fa3d2f17483ae69deca32bc0f321c54b8679  kernel-rv-final
d6e27da798b5f7611e648454e1aa3f3f7e26b6d925528792e3891fb317f62dbc  kernel-la-final
```

## 基线性能中位数

```text
RV 8 核 16G: 562.63
LA 12 核 16G: 538.64
```

## 缓存失效不变量

后续 VFS 缓存必须精确处理：

- rename/renameat2
- unlink/unlinkat
- mkdir/rmdir
- mount/umount
- chdir/chroot
- 文件内容或元数据写回

禁止用纯 TTL 代替失效。

