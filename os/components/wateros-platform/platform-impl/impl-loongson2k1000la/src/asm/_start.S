# 2K1000LA PMON/uImage 入口：PMON 加载 uImage 后跳转到 _start。
# arch boot ABI: $r4 = 逻辑 CPU id（读 CSR 0x20/CPUNUM）；$r5/$r6/$r7 = 透传参数
# （PMON 不携带 UEFI argc/argv/envp，内核忽略）。
# U-Boot 2022.04 上 load/entry 落在 0x9000000098000000 窗口，避开
# 物理 0x90000000 处的 U-Boot lock-cache 保留区。
    .section .text.entry.platform
    .globl _start
    .align 2
_start:
    /* 早期 MMIO 串口探针：在进入 Rust 前先写一个 'W'，用于区分
     * “U-Boot 未跳转”与“WaterOS 进入后在初始化早期挂死”。 */
    li.d $t0, 0x800000001fe20000
1:
    ld.bu $t1, $t0, 5
    andi $t1, $t1, 0x20
    beqz $t1, 1b
    ori $t1, $zero, 0x57
    st.b $t1, $t0, 0

    /* 进入内核后先切到直接地址翻译模式（CRMD.DA=1, PG=0）。
     * U-Boot 跳转前处于 PG=1/DA=0；若不切，高 cached 段的数据访问会走
     * TLB 并触发 refill。 */
    csrrd $t0, 0x0
    li.d $t1, -17
    and $t0, $t0, $t1
    ori $t0, $t0, 0x8
    csrwr $t0, 0x0

    move $r7, $r6
    move $r6, $r5
    move $r5, $r4
    csrrd $r4, 0x20
    andi $r4, $r4, 0x1ff
    b __wateros_arch_boot

    .section .text.entry.platform
    .globl __wateros_get_kernel_high_base
    .type __wateros_get_kernel_high_base, @function
__wateros_get_kernel_high_base:
    li.d $a0, 0x9000000000000000
    jr $ra

    .globl wateros_kernel_main
    .type wateros_kernel_main, @function
wateros_kernel_main:
    /* 在真正进入 Rust 函数前再写一个 'S'，确认 boot.S 的 bl 落点正确。 */
    li.d $t0, 0x800000001fe20000
1:
    ld.bu $t1, $t0, 5
    andi $t1, $t1, 0x20
    beqz $t1, 1b
    ori $t1, $zero, 0x53
    st.b $t1, $t0, 0
    la.global $t0, wateros_kernel_main_rust
    jr $t0

    .globl __wateros_platform_boot_marker
    .type __wateros_platform_boot_marker, @function
__wateros_platform_boot_marker:
    li.d $t0, 0x800000001fe20000
1:
    ld.bu $t1, $t0, 5
    andi $t1, $t1, 0x20
    beqz $t1, 1b
    ori $t1, $zero, 0x52
    st.b $t1, $t0, 0
    jr $ra
