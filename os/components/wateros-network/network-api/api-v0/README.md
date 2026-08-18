# Network API v0 开发手册

[Network 总览](../../README.md) · [Network syscall 手册](../../../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/README.md)

该 `no_std` crate 定义协议栈与 VFS/syscall之间的后端无关值类型。它当前只表达 IPv4 TCP/UDP，不含 sockaddr布局、Linux常量/errno、smoltcp handle、VFS fd、网卡对象或等待队列。

## 地址和初始化配置

`Ipv4Endpoint { address:[u8;4], port:u16 }` 的字节数组按人类网络字节顺序保存（如 `[127,0,0,1]`），port是主机整数值；syscall解析 sockaddr_in时负责 big-endian端口转换。`0.0.0.0:0` 可表示未绑定/通配，不等于有效peer，必须结合 SocketState解释。

`NetworkConfig { address,prefix_len,gateway }` 是静态 IPv4配置。实现必须拒绝 prefix_len>32，并决定 gateway=0是否表示无网关；重复初始化返回AlreadyInitialized。它没有DNS、DHCP、多个地址、IPv6或接口索引，新增这些能力不能挤进保留字段。

## SocketKind 与状态机

`SocketKind` 只有 Tcp/Udp。`SocketState` 是WaterOS生命周期，不是smoltcp内部state逐项镜像：

```text
Created -> Bound{port}
TCP: Bound/Created -> Listening{port} 或 Connecting -> Connected -> Closed
UDP: Created/Bound -> Connected（记录默认 peer）或继续无连接收发 -> Closed
```

具体handler必须校验操作允许的state，例如listen只适合TCP且需要可用local port，accept只在Listening，send无destination时UDP需要connected peer。Closed是终态；fd仍存在时操作返回稳定错误而不是访问已删除handle。

`NetworkSocketSnapshot` 供 `/proc/net` 等管理面，一次复制 kind/state/local/peer及tx/rx queue字节数。endpoint在未连接时可能是零值；展示层不能伪造成真实远端。queue是观测快照，不用于poll线性化。

## 一致的 poll 快照

`SocketPollSnapshot` 必须在一个协议栈临界区内生成，使以下字段属于同一时刻：state、can_recv、may_recv、may_send、剩余send_capacity、is_connected、一次性connect_error、pending accept。

- `can_recv`：现在至少有数据可取；
- `may_recv`：未来仍可能收到数据，用于区分暂空与EOF；
- `may_send`：协议状态仍允许发送，不等于当前capacity>0；
- `send_capacity`：剩余可接收字节，不是总buffer；
- `connect_error`：异步connect完成错误，应按SO_ERROR消费/清除策略处理；
- `has_pending_accept`：listen backlog当前有可accept连接。

poll层不能分别多次查询再拼接，否则connect/close在中间变化会同时报告矛盾的POLLIN/POLLOUT/HUP/ERR。拿到snapshot后仍可能有竞争，实际read/write必须再次验证。

## 错误分层

错误分三层：通用 `NetworkError` 用于 bind/connect/listen 等控制操作，`SocketSendError` 表达发送的 WouldBlock/NoBuffer/MessageTooLarge，`SocketRecvError` 表达 reservation Busy/Empty/Finished。聚合层映射为 VFS 错误，syscall 再映射 errno；不要在 API 增加负整数返回。

典型映射需保持区别：WouldBlock→EAGAIN，NoBufferSpace→ENOBUFS，MessageTooLarge→EMSGSIZE，NotConnected→ENOTCONN，InvalidDestination→EDESTADDRREQ。`NetworkError::Unsupported` 通常EOPNOTSUPP，不是ENOSYS；InvalidSocket与fd不存在/非socket仍需VFS层区分EBADF/ENOTSOCK。

`SocketConnectError` 当前只有Refused/TimedOut。它是异步完成结果，不等同connect调用同步返回值；SO_ERROR读取后是否清零要符合Linux。新增NetworkUnreachable等时同步扩展poll、sockopt和errno映射。

## 接收事务

接收预留协议：prepare 只 peek 并建立唯一 reservation，finish 根据真实用户复制进度消费或返回 Fault，cancel/Drop 不消费。UDP 是报文语义，短缓冲仍消费一个完整数据报但只返回 copied 前缀；TCP 是字节流语义，只消费已提交字节。

`SocketRecvError::Busy`表示已有active reservation，Empty表示暂时无数据，Finished表示流已结束/该token已完成的语义由实现明确，NoMemory用于fallible staging。首字节copy fault应返回`SocketRecvFinish::Fault`并不消费；TCP部分copy提交前缀并保留后缀；UDP只要复制了允许的截断前缀，提交时消费整个datagram，与MSG_TRUNC返回长度策略由syscall决定。

reservation必须绑定socket id/generation，finish/cancel恰好一次。不能持协议栈spin锁进行user-copy或scheduler wait；流程为短锁prepare→解锁copy→短锁finish。close/dup/fork时active reservation owner与唤醒策略要明确。

## 增加 socket 能力示例

例如添加新 sockopt：

1. 判断它是协议栈真实参数、只读快照还是仅兼容记录；在 API 中用中立类型表达。
2. 在 smoltcp `SocketMeta` 和 `sockopt.rs` 实现 get/set、默认值和 fork/dup 共享规则。
3. `SocketRef` 暴露操作，VFS handle 仅在通用 I/O 需要时转发。
4. syscall 解析 level/optname 和用户结构，处理长度与端序。
5. 测试 TCP/UDP、非法长度、关闭竞态、dup 共享，以及该参数是否真正改变底层行为。仅存值供 getsockopt 回读时必须明确文档说明。

新增协议族不能硬塞进 `Ipv4Endpoint`/`SocketKind`：应先设计可区分地址族的 API，再修改快照、VFS resource kind、syscall 分流和 `/proc/net`。

## 新增 IPv6 的结构方案

先引入 `IpEndpoint` enum或`SocketAddress { family, bytes, port, scope_id }`，避免用全零IPv4表示IPv6；扩展NetworkConfig为每接口地址列表；给SocketKind/State保持协议与地址族正交。随后同步smoltcp storage、bind冲突、sockaddr_in6、getsockname/peername、proc和poll。旧IPv4 API若保留，需要显式转换失败而非截断。

## 生命周期和锁

socket id/handle通常由协议栈registry拥有，fd/dup共享一个open-file description及flags/offset语义。fork复制Arc引用，exec按CLOEXEC关闭，最后close才删除协议栈handle并唤醒poll/connect/accept/recv waiter。任何snapshot或reservation跨锁使用都必须持generation/owner，防handle槽位复用。

协议栈锁内不进入网卡driver反向回调、VFS、user-copy或scheduler wait。poll worker可在锁内推进一次有界device/socket poll，再释放并统一wake，避免网络锁→waitqueue→task→socket的反向链。

## 回归清单

- IPv4端序、prefix 0/32/33、gateway零值和重复init；
- TCP/UDP每个合法/非法state transition与Closed终态；
- 一次poll snapshot内部一致，connect/close并发不报告矛盾位；
- send 0/partial/full、capacity边界及每种错误到errno；
- TCP/UDP reservation全/部分/首字节EFAULT、Busy、cancel、close竞态；
- UDP短buffer/MSG_TRUNC与TCP字节流差异；
- dup/fork/exec/last close、handle generation复用；
- 大量socket/poll下heap回落、锁持有时间和SMP唤醒无死锁。
