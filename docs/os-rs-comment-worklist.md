# os 目录 Rust 文件工作清单

## 注释工作规范

逐文件检查并完善现有注释，注释正文必须使用简体中文。所有具有对外接口、语义、约束或设计意义的模块、类型、trait、枚举、字段、常量、静态量、函数、方法、参数、返回值、错误、非显然局部变量和关键状态都必须有准确说明。说明至少覆盖：功能与使用方式、参数/返回值及单位、字段含义、错误语义、资源生命周期、并发与锁约束、`unsafe` 安全不变量、边界条件与异常值处理，以及必要的设计原因。除方法级注释外，还必须在方法内部为关键处理流程补充行内说明：解释重要调用为何发生、调用顺序为何如此、分支条件对应的状态或协议、数据/状态如何变化、正常值之外的零值/空值/最大值/溢出/未对齐/非法标志等输入如何处理、错误如何传播以及失败后的状态是否回滚或保持不变，以及锁、屏障、唤醒、回滚等顺序约束；不能只描述“做了什么”，还要说明“为什么这样做”。已有注释必须逐条核对，过时、含糊或不完整的内容必须修正，不能因“已有注释”而跳过。简单且不会引起误解的局部变量不机械添加冗余注释。每完成一个文件，立即将其清单项改为删除线形式 `~~路径~~（已完成）`，保留路径以便审计；不得标记未完成或仅编译通过的文件。

固定审查范围：

- 模块、公共接口、trait、结构体、枚举及其字段；
- 函数、方法、参数、返回值、错误和使用前提；
- 常量、静态变量、全局状态及其生命周期；
- 非显然的局部变量、状态字段和关键中间数据；
- 地址、长度、标志位、时间单位等字段的具体含义；
- 锁、并发、原子操作、资源所有权和生命周期约束；
- `unsafe` 代码的安全不变量与调用者责任；
- 边界条件、异常值、溢出/下溢、空值、零值、最大值、未对齐地址、非法标志和失败后的状态恢复；
- 系统调用、页表、调度、文件系统、驱动等内核机制的设计原因；
- 已有注释的准确性、完整性和是否仍符合当前实现。

本清单由 `rg --files os -g '*.rs' -g '!os/vendor/**' | sort` 生成，共 535 个文件。后续注释审阅与补充以此清单为准，按文件逐条处理。`os/vendor/` 下的第三方源码不在本次工作范围内。

- ~~`os/build.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/fs.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/ipc.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/klog.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/mm.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/syscall.rs`~~（已完成）
- ~~`os/components/wateros-base/base-config/src/task.rs`~~（已完成）
- ~~`os/components/wateros-base/src/cpu.rs`~~（已完成）
- ~~`os/components/wateros-base/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-base/src/sync/mod.rs`~~（已完成）
- ~~`os/components/wateros-base/src/sync/multiprocessor.rs`~~（已完成）
- ~~`os/components/wateros-base/src/sync/once.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-api/api-v0/src/traits.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-api/api-v0/src/types.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-impl/impl-root/src/hooks.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-impl/impl-root/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-cred/cred-impl/impl-root/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-cred/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-debug/build.rs`~~（已完成）
- ~~`os/components/wateros-debug/src/events.rs`~~（已完成）
- ~~`os/components/wateros-debug/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-debug/src/locks.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/device.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/index.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/manager.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/block-impl/impl-virtio-pci/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-block/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-character/character-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-character/character-impl/impl-null-stub/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-character/character-impl/impl-rtc-stub/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-character/character-impl/impl-uart-16550/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-character/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-display/display-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-display/display-impl/impl-virtio-mmio/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-display/display-impl/impl-virtio-pci/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-display/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-common/src/dtb.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-common/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-common/src/virtio_hal.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/devfs.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/enumerate.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/machine.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/register.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/test.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/uart.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/devfs.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/enumerate.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/machine.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/register.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/test.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-impl/impl-qemu-riscv64-virt/src/uart.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-input/input-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-input/input-impl/impl-virtio-mmio/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-input/input-impl/impl-virtio-pci/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-input/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-network/network-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-network/network-impl/impl-virtio-mmio/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-network/network-impl/impl-virtio-pci/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/driver-network/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-driver/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-api/api-v0/src/handles.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-api/api-v0/src/traits.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-api/api-v0/src/types.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/devfs-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/aliases.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/fs_impl.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/manager.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-devfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/backend.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/block_io.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/dentry_cache.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/filesystem.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/operations.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/path_lookup.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-another-ext4/src/positive_dentry_cache.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-devfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4-rs/src/core.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4-rs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4-rs/src/operations.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4/src/boot_inspect.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4/src/ro.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ext4/src/selftest.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ramfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ramfs/src/tree/operations.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-impl/impl-ramfs/src/tree.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/callbacks.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/fs_impl.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/path.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/render.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/view.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-procfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/rootfs-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/mount.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/state.rs`~~（已完成）
- ~~`os/components/wateros-fs/fs-rootfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-fs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/color.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/event.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/geometry.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/text.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-api/api-v0/src/widget.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/canvas.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/demo.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/font.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/global.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/input.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/runtime.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/scene.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/surface.rs`~~（已完成）
- ~~`os/components/wateros-gui/gui-impl/impl-software/src/theme.rs`~~（已完成）
- ~~`os/components/wateros-gui/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-event/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/error.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/key.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/robust.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/wait.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/global.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-futex/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/endpoint.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/error.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/kernel_pipe.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/endpoint.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-pipe/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-impl/impl-frame/src/allocation.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-impl/impl-frame/src/global.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-impl/impl-frame/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-impl/impl-frame/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/shm-impl/impl-frame/src/state.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-shm/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/global.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/state.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/timer.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-signal/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-waitqueue/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-waitqueue/waitqueue-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/ipc-waitqueue/waitqueue-impl/impl-task/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-ipc/src/lib.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/action.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/error.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/flags.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/meta.rs`~~（已完成）
~~`os/components/wateros-klog/klog-api/api-v0/src/store.rs`~~（已完成）
~~`os/components/wateros-klog/klog-impl/impl-kernel/src/format.rs`~~（已完成）
~~`os/components/wateros-klog/klog-impl/impl-kernel/src/global.rs`~~（已完成）
~~`os/components/wateros-klog/klog-impl/impl-kernel/src/lib.rs`~~（已完成）
~~`os/components/wateros-klog/klog-impl/impl-kernel/src/state.rs`~~（已完成）
~~`os/components/wateros-klog/klog-impl/impl-kernel/src/syslog.rs`~~（已完成）
~~`os/components/wateros-klog/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/address_space.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/addr.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/brk.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/elf_user_stack.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/error.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/executable.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/flags.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/frame_allocator.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/kernel_bringup.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/kernel_satp.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/mempolicy.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/mmap.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/perm.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/user_access.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/user_aspace_lifecycle.rs`~~（已完成）
~~`os/components/wateros-mm/mm-api/api-v0/src/user_mapping.rs`~~（已完成）
~~`os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-frame-alloctor/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/cache.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/elf.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/fault.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/mapping.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/common/src/vma.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/asid.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_elf.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_executable.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/kernel_global.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_access.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_aspace.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_heap_mmap.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/asid.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_executable.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_global.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/user_aspace.rs`~~（已完成）
~~`os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`~~（已完成）
~~`os/components/wateros-mm/src/kernel_mm.rs`~~（已完成）
~~`os/components/wateros-mm/src/lib.rs`~~（已完成）
~~`os/components/wateros-mm/src/mempolicy.rs`~~（已完成）
~~`os/components/wateros-network/network-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/adapter.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/lib.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/global.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/init.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/mod.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/poll.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/receive.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/socket.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/sockopt.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/state.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/tcp.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/types.rs`~~（已完成）
~~`os/components/wateros-network/network-impl/impl-smoltcp/src/stack/udp.rs`~~（已完成）
~~`os/components/wateros-network/src/lib.rs`~~（已完成）
~~`os/components/wateros-network/src/socket/fd.rs`~~（已完成）
~~`os/components/wateros-network/src/socket/lease.rs`~~（已完成）
~~`os/components/wateros-network/src/socket/mod.rs`~~（已完成）
~~`os/components/wateros-network/src/socket/object.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/boot.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/console.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/reset.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/smp.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/timer.rs`~~（已完成）
~~`os/components/wateros-platform/platform-api/api-v0/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/cpu.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/interrupt.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/kernel_trap.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/paging.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/task.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-api/api-v0/src/trap.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/cpu.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/interrupt.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/paging.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/task.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/trap.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/cpu.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/interrupt.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/ipi.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/paging.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/task.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`~~（已完成）
~~`os/components/wateros-platform/platform-arch/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/boot.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/console.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/dtb.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/memory.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/reset.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/smp.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/timer.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/boot.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/console.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/dtb.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/memory.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/reset.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/smp.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/timer.rs`~~（已完成）
~~`os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/src/boot.rs`~~（已完成）
~~`os/components/wateros-platform/src/console.rs`~~（已完成）
~~`os/components/wateros-platform/src/lib.rs`~~（已完成）
~~`os/components/wateros-platform/src/smp.rs`~~（已完成）
~~`os/components/wateros-platform/src/timer.rs`~~（已完成）
~~`os/components/wateros-platform/src/time.rs`~~（已完成）
~~`os/components/wateros-platform/src/wall_clock.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-console/console-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-console/console-impl/impl-platform-console/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-console/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-heap-allocator/src/backend_linked_list.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-heap-allocator/src/stress.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-logging/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-logging/src/logger.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-panic/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/runtime-serial/src/lib.rs`~~（已完成）
~~`os/components/wateros-runtime/src/lib.rs`~~（已完成）
~~`os/components/wateros-syscall/src/lib.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-api/api-v0/src/args.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-api/api-v0/src/errno.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-api/api-v0/src/number.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-api/api-v0/src/return_value.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/epoll_fd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/fallible_buf.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/linux_stat.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/mm_util.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_block.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_fd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/stat_times.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/cap.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/groups.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/setid.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/attr.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/close.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/cwd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dup.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/faccessat.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fadvise.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fallocate.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fcntl.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/flock.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/getdents64.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/inotify.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/memfd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat2.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/path_at.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/pipe2.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/renameat2.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/sendfile.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/statfs.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/transfer.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/truncate.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/xattr.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/futex.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/kill_target.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/robust.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/shm.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signalfd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/sysv_msg.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/sysv_sem.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/brk.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mempolicy.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mincore.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/acct.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/bringup_stats.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/mount.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/reboot.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/riscv_flush_icache.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/riscv_hwprobe.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/sync.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/sysinfo.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/syslog.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/umount2.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/accept.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/bind.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/connect.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/listen.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/recvfrom.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sendmsg.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sendto.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/shutdown.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socketpair.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socket.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sockname.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sockopt.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/epoll.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/poll_multiplex.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/poll/poll.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/clone.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/ioprio.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/personality.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/pidfd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/priority.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/process.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/rlimit.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/rseq.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/unshare.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/vfork.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/clock.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/mod.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/posix_timer.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/rtc.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/timerfd.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/timer.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`~~（已完成）
~~`os/components/wateros-syscall/syscall-impl/impl-kernel/src/vfs_util.rs`~~（已完成）
~~`os/components/wateros-task/src/cpu.rs`~~（已完成）
~~`os/components/wateros-task/src/lib.rs`~~（已完成）
~~`os/components/wateros-task/src/lifecycle.rs`~~（已完成）
~~`os/components/wateros-task/src/process.rs`~~（已完成）
~~`os/components/wateros-task/src/runtime.rs`~~（已完成）
~~`os/components/wateros-task/src/sched.rs`~~（已完成）
~~`os/components/wateros-task/src/schedule.rs`~~（已完成）
~~`os/components/wateros-task/src/spawn.rs`~~（已完成）
~~`os/components/wateros-task/src/trap.rs`~~（已完成）
~~`os/components/wateros-task/src/wait_queue.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/kernel.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/process.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/sched.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/snapshot.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/task.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/user.rs`~~（已完成）
~~`os/components/wateros-task/task-api/api-v0/src/wait.rs`~~（已完成）
~~`os/components/wateros-task/task-impl/impl-core/src/lib.rs`~~（已完成）
~~`os/components/wateros-task/task-impl/impl-core/src/process.rs`~~（已完成）
~~`os/components/wateros-task/task-impl/impl-core/src/tcb.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/cfs_queue.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/cpu.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/fifo_queue.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/rr_queue.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/cpu.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/policy.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/query.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/runqueue.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/tasks.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/wait.rs`~~（已完成）
~~`os/components/wateros-task/task-scheduler/src/lib.rs`~~（已完成）
~~`os/components/wateros-tty/src/lib.rs`~~（已完成）
~~`os/components/wateros-tty/tty-api/api-v0/src/lib.rs`~~（已完成）
~~`os/components/wateros-tty/tty-impl/impl-console/src/lib.rs`~~（已完成）
~~`os/components/wateros-tty/tty-impl/impl-console/src/pty.rs`~~（已完成）
~~`os/components/wateros-utils/src/lib.rs`~~（已完成）
~~`os/components/wateros-utils/table-format/src/auto_table.rs`~~（已完成）
~~`os/components/wateros-utils/table-format/src/fixed_table.rs`~~（已完成）
~~`os/components/wateros-utils/table-format/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/cwd.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/fd.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/mount_ns.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/mount.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/root.rs`~~（已完成）
- ~~`os/components/wateros-vfs/src/self_test.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/backend.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/dev.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/error.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/fd.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`~~（已完成）
~~`os/components/wateros-vfs/vfs-api/api-v0/src/kind.rs`~~（已完成）
~~`os/components/wateros-vfs/vfs-api/api-v0/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/meta.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/mount.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/namespace.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/path.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/resolve.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/root_read.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-api/api-v0/src/rw_session.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/char_dev_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/cwd.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/file_lock.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/interrupt_guard.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/pty.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fd-session/src/user_graphics.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/dir_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_ns.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/path_ops.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/proc_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/read_lease.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/read_view.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/stable_node.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/symlink_handle.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/sysfs.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/tmpfs.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/cache_state.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/file_cache.rs`~~（已完成）
- ~~`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`~~（已完成）
~~`os/src/boot_timebase.rs`~~（已完成）
~~`os/src/dashboard.rs`~~（已完成）
~~`os/src/debug_fault.rs`~~（已完成）
~~`os/src/main.rs`~~（已完成）
~~`os/src/stall_debug.rs`~~（已完成）
~~`os/src/trap_handler.rs`~~（已完成）
~~`os/src/user_bringup_bus.rs`~~（已完成）
~~`os/src/user_bringup_busybox.rs`~~（已完成）
~~`os/src/user_bringup_common.rs`~~（已完成）
~~`os/src/user_bringup_ltp_exclusions.rs`~~（已完成）
~~`os/src/user_bringup_mm.rs`~~（已完成）
~~`os/src/user_bringup_posix_fs.rs`~~（已完成）
~~`os/src/user_bringup_root_layout.rs`~~（已完成）
~~`os/src/user_operator.rs`~~（已完成）
