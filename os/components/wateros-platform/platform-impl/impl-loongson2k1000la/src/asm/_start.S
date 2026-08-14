# 2K1000LA PMON/uImage 入口：PMON 加载 uImage 后跳转到 _start。
# arch boot ABI: $r4 = 逻辑 CPU id（读 CSR 0x20/CPUNUM）；$r5/$r6/$r7 = 透传参数
# （PMON 不携带 UEFI argc/argv/envp，内核忽略）。
# TODO(real-hardware): 对照实际 PMON 加载地址（0x9000000090000000 窗口）验证。
    .section .text.entry.platform
    .globl _start
    .align 2
_start:
    move $r7, $r6
    move $r6, $r5
    move $r5, $r4
    csrrd $r4, 0x20
    b __wateros_arch_boot
