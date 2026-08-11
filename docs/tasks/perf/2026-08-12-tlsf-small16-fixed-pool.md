# BuildStorm 16 字节固定对象池实验

## 问题与画像

current main 的 300 秒 allocator 画像中，16 字节及以下布局是最集中的一类：共出现
5,580,621 次分配、4,753,953 次释放和 367,338 次 realloc；实际申请字节合计
38,899,109，平均每次约 7 字节。同期 TLSF 的锁进入 31,094,581 次，但竞争仅
718,213 次（2.3%），说明主要成本不是锁等待，而是每个极小对象仍进入通用 TLSF 的
位图、空闲链和块元数据路径。

此前的八尺寸级 slab 候选覆盖到 1024 字节，增加了 span、中心回收和多级分支，最终
910.08s，比当时对照慢 3.37%；另一个 allocator guard 局部深度候选也没有改善。因此本轮
不重做通用 slab，也不混入 interrupt guard 改造，只验证一个单一尺寸级能否以足够小的热路径
绕过 TLSF。

## 候选方案

从现有 128 MiB `HEAP_SPACE` 的起始处保留 16 MiB，划分为 1,048,576 个 16 字节槽；
剩余 112 MiB 继续作为 TLSF pool，总堆容量不变。只有 `size <= 16 && align <= 16` 的非零
布局进入固定池。

每个 CPU 只维护两类本地状态：

- 单链空闲表，next 指针直接存入已经释放的 16 字节槽；
- 从全局原子 bump 游标一次领取 64 个槽的本地区间。

分配先弹出本地空闲表，再使用本地 bump 区间；两者都没有可用槽时才领取新批次。释放到
当前 CPU 的本地空闲表，允许对象在另一个 CPU 上释放并转移所有权。allocator 原有的本 CPU
关中断 guard 保证本地状态不会被中断重入，不新增中心锁、span header、IPI 或排空协议。
全局固定池耗尽时回退到 TLSF，不把池容量选择变成功能正确性的前提。

固定池地址范围与 TLSF 地址范围严格分离。dealloc/realloc 对落在固定池中的指针验证槽对齐和
原布局资格，非法指针沿用 allocator 的拒绝路径，绝不能误交给 TLSF。固定池 realloc 在新布局
仍符合 16 字节限制时原地返回；扩容时通过正常 allocator 路径分配、复制并释放旧槽，避免在
allocator guard 内递归进入 `GlobalAlloc`。

## 为什么可能有效

Linux 的 per-CPU slab 快路径通过 CPU-local freelist 避免通用页分配器和全局同步；本候选只学习
这一点，不复制完整 SLUB 架构。64 槽批量领取把最坏约 558 万次小对象分配对应的全局原子操作
压低约两个数量级，而复用本地 free slot 时完全不触碰 TLSF mutex 和 TLSF 元数据。

固定 16 MiB 来自 300 秒画像的保守上界：alloc-free 的差值约 826,668 个槽（约 12.6 MiB），
其中尚未扣除由 realloc 隐含释放的旧对象；若长程并发活跃对象超过该容量，回退机制仍保证
正确性。每 CPU 最多滞留 63 个尚未使用的批量槽，32 CPU 下不足 32 KiB。

## 实施与验证

1. 新增独立 `small_pool` 模块，保持 linked-list allocator feature 完全不变。
2. TLSF 初始化时跳过固定池范围；统计把固定槽实际占用按 16 字节计入 used/free。
3. 增加布局资格、边界、批次末尾和错误指针的定向测试；执行受影响 crate 的检查。
4. 构建 RV/LA Final，确认 `make all` 的默认别名仍与 Final 相同，并保留脚本正文打印 marker。
5. 只运行一轮同镜像 BuildStorm candidate，与 current-best 783.00s 比较。若首次明确改善则接受；
   若持平或回退则停止，不用第二次运行制造结论。

## 接受条件

候选必须通过所有 BuildStorm marker、无 panic/stall，并给出超出近期约 10--13 秒自然抖动的明确
改善，才可合入 main。即使指令画像显示 TLSF 热点下降，只要墙钟没有达到这一条件也判定失败。

## 首轮长程失败与方案收缩

16 MiB 候选通过双架构构建、toolchain 和 minibuild，但在 BuildStorm 239.44s 时失败。rustc 在写
`lib.rmeta` 时需要一块 3,670,016 字节、8 字节对齐的分配；当时统计为 used=104,657,177、
free=29,560,551，TLSF 仍无法满足连续大块并触发 OOM。结果文件为
`/tmp/wateros-buildstorm-fixed/tlsf-small16-fixed-pool-a1/result.json`。这证明总空闲量足够并不代表
把 16 MiB 永久隔离后仍有足够的 TLSF 连续空间，首轮不能作为性能结果。

候选因此收缩为 4 MiB（262,144 槽），TLSF 从 112 MiB 恢复到 124 MiB。固定池不再试图容纳
画像中的全部瞬时净分配，只服务可以在各 CPU freelist 上反复周转的短命小对象；池满后的新对象
继续回退 TLSF。这个改动保留绕过通用分配路径的假设，同时把永久隔离量降到总堆的 3.125%。
