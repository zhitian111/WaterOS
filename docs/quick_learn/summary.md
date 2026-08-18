
futex：

线程先在用户态通过 CAS 读写一个 32 位锁字来尝试获取锁。CAS 成功说明锁原本空闲，线程直接获得锁，不进入内核；CAS 失败说明锁已被其他线程持有，当前线程就对这个锁字地址调用 `futex_wait`。

内核根据锁字地址、地址空间以及是否共享映射生成 `FutexKey`。每个正在使用的 `FutexKey` 在内核中对应一个 `WaitQueue`；等待线程会被挂到该队列上并阻塞。持锁线程释放锁时，先在用户态把锁字改为未锁定，再调用 `futex_wake`，内核据此找到对应的等待队列并唤醒一个或多个线程。被唤醒的线程不会直接拥有锁，而是重新通过 CAS 竞争锁。

为了避免“等待线程完成检查后、真正进入等待队列前，解锁线程已经完成 wake”的丢失唤醒问题，WaterOS 用 `wake_sequence` 记录唤醒序号：等待线程在登记时记录当前序号，真正睡眠前再次检查；如果序号已经变化，就不再睡眠。

普通 futex 有一个问题：如果持锁线程突然退出或崩溃，它无法执行“解锁 + wake”，等待该锁的线程可能永久阻塞。

robust futex 用来处理这个问题。线程在用户态维护一条 robust 链表，记录自己当前持有的 robust 锁；线程通过 `set_robust_list` 将这条链表头及所属地址空间登记到内核。WaterOS 内核的 registry 不保存每一个锁对应的 `FutexKey`，而是保存“线程 -> robust 链表登记信息”。

当线程退出时，内核读取并遍历该线程的用户态 robust 链表，找到它持有的每个 futex 锁字。如果锁字中的 owner TID 确实是这个已退出线程，内核会清除 owner TID、设置 `FUTEX_OWNER_DIED` 标志；若锁字带有 `FUTEX_WAITERS` 标志，则根据该锁字地址解析出对应的 `FutexKey`，并唤醒一个等待线程。

被唤醒的线程重新获得锁时会发现 `FUTEX_OWNER_DIED`，从而知道前一个持锁者异常退出，受保护的数据可能不一致；它需要先完成恢复处理，再将锁恢复到正常状态。

signal

signal：当一个信号产生时——它可能来自 `kill` 等 syscall，也可能来自 CPU 异常、定时器、终端或内核事件——内核将对应信号标记加入目标线程或进程的 pending 集合。若该信号可交付，内核会唤醒或中断合适的目标线程。

在线程即将从内核返回用户态的安全点，内核检查该线程的 pending 与进程 pending，并结合该线程的 signal mask 和进程的 `sigaction` 判断如何处理信号：忽略、终止进程、停止/继续进程，或调用用户注册的 handler。

若要调用 handler，内核会在用户栈或备用信号栈上构造 signal frame，其中保存被中断时的寄存器、PC、原 signal mask、`siginfo` 和 `ucontext`；随后修改 trap frame，使线程返回用户态时从 handler 函数开始执行。

handler 执行结束后会跳到 signal trampoline，由它发起 `rt_sigreturn` syscall；内核读取 signal frame，恢复原先的寄存器、PC 和 signal mask，最终回到被信号打断的用户态执行位置继续运行。
