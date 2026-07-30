# runtime-panic

`panic_handler` 输出 panic 位置和消息，尝试 flush console，然后调用 platform shutdown。
firmware 若返回或不支持 shutdown，handler 将持续重试并最终停在无限循环，保证不会从
`#[panic_handler]` 返回。

panic 路径是 best-effort：不得依赖 heap、scheduler、VFS 或可阻塞锁已经可用。早期启动
阶段可能没有可见输出，但仍必须进入终止路径。
