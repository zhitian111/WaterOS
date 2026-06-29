#!/usr/bin/env python3
"""为各 .tex 文件顶部写入 LaTeX 注释（以 % 开头），说明本文件应写什么；不删除正文。"""

from __future__ import annotations

import re
from pathlib import Path

DOC_ROOT = Path(__file__).resolve().parent.parent
CHAPTERS = DOC_ROOT / "chapters"
COMPONENTS = CHAPTERS / "chap03" / "components"
FRONTMATTER = DOC_ROOT / "frontmatter"
OS_COMPONENTS = "os/components"

# 一级组件综述（section 级 .tex）
COMPONENT_HINTS: dict[str, str] = {
    "wateros-abi": "用户态与内核 ABI 约定：系统调用号、寄存器/栈约定、错误码映射。",
    "wateros-base": "内核基础类型、地址表示与全局配置常量。",
    "wateros-cred": "进程凭证（UID/GID/CAP）会话模型与生命周期 hook。",
    "wateros-driver": "块设备、字符设备、网络设备与板级驱动聚合入口。",
    "wateros-fs": "文件系统聚合：devfs/procfs/rootfs 与块设备 ext4 根卷。",
    "wateros-ipc": "进程间通信：等待队列、管道、futex、共享内存、信号等。",
    "wateros-klog": "内核持久化日志环与 syslog(2) 对接。",
    "wateros-mm": "虚拟内存、页表、物理帧分配与用户态映射。",
    "wateros-platform": "架构抽象、固件/板级与 trap/定时器平台能力。",
    "wateros-pseudo-shell": "内核内嵌伪 shell，用于 bring-up 交互调试。",
    "wateros-runtime": "控制台、分级日志、panic、堆分配器与串口再导出。",
    "wateros-syscall": "系统调用分发表与 trap 入口到各子系统的接线。",
    "wateros-task": "任务对象、调度器与内核线程生命周期。",
    "wateros-utils": "跨组件通用工具（容器、位图、字符串等）。",
    "wateros-vfs": "虚拟文件系统：fd 表、页缓存、与 fs 桥接。",
}

# 子模块 / impl 补充说明（键为目录名或 impl 文件名 stem）
SUBMODULE_HINTS: dict[str, str] = {
    "abi-api": "ABI 顶层 api-v0 契约 crate。",
    "abi-impl": "ABI 实现容器：dummy 占位与 linux-generic64 主线。",
    "base-config": "base 配置子 crate，无 api/impl 分叉。",
    "driver-api": "驱动聚合层 api-v0。",
    "driver-block": "块设备子系统：VirtIO、块缓存与设备枚举。",
    "driver-character": "字符设备：UART、RTC 等 stub/真实后端。",
    "driver-network": "网络设备与 smoltcp 协议栈对接。",
    "driver-impl": "板级驱动 impl：QEMU RISC-V / LoongArch 与 dummy。",
    "fs-api": "文件系统顶层 FsImpl 注册表契约。",
    "fs-devfs": "设备节点伪文件系统。",
    "fs-procfs": "进程信息伪文件系统。",
    "fs-rootfs": "根卷挂载与当前根句柄。",
    "fs-impl": "块 FS 实现容器：ext4-rs（默认）、ext4 旧路径等。",
    "ipc-api": "IPC 顶层 api-v0（当前多为占位）。",
    "ipc-waitqueue": "对 task::WaitQueue 的 IPC 命名空间薄包装。",
    "ipc-pipe": "内核 ring-buffer 管道与 fd 端点。",
    "ipc-futex": "Futex 等待/唤醒与 robust 侧表。",
    "ipc-shm": "SysV 共享内存段注册表（单 crate，无 api/impl 树）。",
    "ipc-signal": "进程/线程信号状态机与 itimer。",
    "ipc-event": "事件 IPC 占位 crate。",
    "ipc-impl": "IPC 聚合层 active_impl 占位（impl-dummy）。",
    "klog-api": "klog api-v0：记录描述符与环读写契约。",
    "klog-impl": "klog 存储后端（ringbuf）。",
    "mm-api": "内存管理 api-v0：映射、页表操作 trait。",
    "mm-frame-alloctor": "物理页帧分配器子系统。",
    "mm-impl": "页表实现：Sv39、LoongArch、dummy 与 common 共享代码。",
    "platform-api": "平台聚合 api-v0。",
    "platform-arch": "架构相关：trap、上下文切换、定时器、SMP。",
    "platform-impl": "板级/QEMU 平台 impl 选路。",
    "runtime-console": "控制台输出后端与 firmware 对接。",
    "runtime-heap-allocator": "内核堆：buddy（默认）/ TLSF。",
    "runtime-logging": "log! 宏分级过滤（与 klog 独立）。",
    "runtime-panic": "panic 处理与回溯输出路径。",
    "runtime-serial": "串口再导出（委托 driver UART）。",
    "syscall-api": "系统调用 api-v0：分发接口与错误类型。",
    "syscall-impl": "内核侧 syscall 实现（impl-kernel）。",
    "task-api": "任务 api-v0：TaskId、KernelTask、上下文。",
    "task-impl": "任务核心实现（impl-core）：TCB、registry。",
    "task-scheduler": "调度器 api 与 impl（multi-class / round-robin）。",
    "vfs-api": "VFS api-v0：inode、fd、路径解析契约。",
    "vfs-impl": "VFS 后端：fd-session、fs-bridge、page-cache、dummy。",
    # impl variant stems
    "dummy": "占位/链接桩实现，保证默认 feature 可编译。",
    "linux-generic64": "Linux 兼容 generic64 ABI 实现（主线）。",
    "root": "cred 根会话实现，与 syscall cred-session 对接。",
    "qemu-riscv64-opensbi": "QEMU RISC-V + OpenSBI 板级组合。",
    "qemu-loongarch64-virt": "QEMU LoongArch virt 机器组合。",
    "block-cache": "块设备读缓存层。",
    "virtio-mmio": "VirtIO MMIO 传输（RISC-V 主线块/网）。",
    "virtio-pci": "VirtIO PCI 传输（LoongArch 主线）。",
    "null-stub": "空字符设备 stub。",
    "rtc-stub": "RTC 字符设备 stub。",
    "smoltcp": "smoltcp 网络协议栈 impl。",
    "ext4": "旧 ext4plus 实现路径（feature impl-ext4）。",
    "ext4-rs": "默认 ext4 根卷 RW 实现（impl-ext4-rs）。",
    "kernel": "内核内建伪 FS / rootfs 逻辑（devfs、procfs、rootfs）。",
    "devfs": "devfs 作为 fs-impl 注册项的说明。",
    "ringbuf": "pipe ring-buffer 实现。",
    "task": "委托 wateros-task 的等待/唤醒实现。",
    "multi-class": "多级就绪队列调度策略。",
    "round-robin": "时间片轮转调度策略。",
    "core": "任务对象、启动与 runtime 机制核心。",
    "sv39": "RISC-V Sv39 页表实现。",
    "loongarch64": "LoongArch 页表 / 架构相关 impl。",
    "common": "mm-impl 跨平台共享辅助代码（非可选变体）。",
    "stack": "物理帧栈式分配器（默认）。",
    "riscv64": "RISC-V 架构 trap/上下文 platform-arch impl。",
    "platform-console": "经 platform 固件/控制台的输出后端。",
    "fd-session": "进程 fd 表、cwd 与 session 生命周期。",
    "fs-bridge": "VFS 与 wateros-fs 桥接后端。",
    "page-cache": "VFS 页缓存层。",
}

CHAPTER_FILES: dict[str, list[str]] = {
    "main.tex": [
        "文档入口：组织封面、目录与五章 \\include。",
        "一般无需写正文；调整章节顺序或增删 \\include 时改此文件。",
        "版式与宏：setup/package.tex、setup/format.tex、setup/doc-macros.tex。",
    ],
    "frontmatter/cover-page.tex": [
        "封面页（titlepage）：插图、文档标题、参赛队名/成员/指导老师。",
        "修改 \\todo{待填写} 为实际信息；换封面图请替换 figures/cover.jpg。",
    ],
    "chap01.tex": [
        "第 1 章「项目概述」。",
        "",
        "§1.1 设计目标：工程目标、no_std、双架构、Linux ABI 对齐范围。",
        "§1.2 当前实现摘要：用表格概括各子系统现状（可随实现更新）。",
        "",
        "当前正文来自 test.tex；修订时保持与 docs/exports/snapshot/ 一致。",
        "事实来源：docs/exports/snapshot/current.md、os/feature-tree.txt",
    ],
    "chap02/written-architecture.tex": [
        "第 2 章「总体架构设计」（当前 main.tex 使用的整章正文）。",
        "",
        "应包含：组件化分层、os/ 代码结构、组件职责表、API/impl 解耦、",
        "      启动主线、Feature 组合、双架构对比、kernel_main 代码流程。",
        "",
        "后续若拆到 chap02/design-philosophy.tex 等子文件，",
        "可将本节内容迁移后改 main.tex 为 \\input 聚合。",
        "事实来源：docs/prompts/architecture.md、os/Cargo.toml、os/src/main.rs",
    ],
    "chap02/architecture.tex": [
        "（备用）第 2 章聚合壳：\\chapter + 多个 \\input 子节。",
        "当前未编入 main.tex；模块化拆分架构章时使用。",
    ],
    "chap02/design-philosophy.tex": [
        "（备用）§ 组件化与语义抽象。",
        "当前未编入 main.tex；内容见 written-architecture.tex 对应节。",
    ],
    "chap02/feature-tree.tex": [
        "（备用）§ Feature 树与编译期选路。",
    ],
    "chap02/api-layers.tex": [
        "（备用）§ 各级 API 设计。",
    ],
    "chap02/aggregation.tex": [
        "（备用）§ 聚合层与原语组合。",
    ],
    "chap03/written-implementation.tex": [
        "第 3 章「关键模块实现」（当前 main.tex 使用的整章正文）。",
        "",
        "按用户态运行路径写：平台/运行时、trap、MM、task、driver、FS/VFS、",
        "syscall、IPC/cred/klog；含代码摘录与数据流说明。",
        "",
        "长期目标：将各节迁入 chapters/chap03/components/ 下对应组件 .tex，",
        "完成后 main.tex 改为 \\include{chapters/chap03/implementation}。",
    ],
    "chap03/implementation.tex": [
        "（目标态）第 3 章模块化聚合：\\chapter + 各一级组件 \\input。",
        "当前未编入 main.tex；各叶子 .tex 在 components/ 下，待从",
        "written-implementation.tex 迁入正文。",
    ],
    "chap04.tex": [
        "第 4 章「测试、复现与问题处理」。",
        "",
        "§4.1 构建与启动：Makefile 目标、双架构构建与 QEMU 运行。",
        "§4.2 功能验证：bring-up 总线、脚本集合、日志判读。",
        "§4.3 双架构一致性验证。",
        "§4.4 遇到的问题和解决方法（按主题分 \\subsection）。",
        "",
        "当前正文来自 test.tex；事实来源：os/Makefile、docs/tasks/",
    ],
    "chap05.tex": [
        "第 5 章「总结与后续工作」。",
        "",
        "§5.1 工作总结与项目特色。",
        "§5.2 后续完善方向。",
        "§5.3 非本队来源说明。",
        "§5.4 AI 工具使用说明（占位，待填写）。",
        "",
        "事实来源：docs/roadmap/todolist.md、docs/exports/ai-usage-inventory.tsv",
    ],
}


def block(lines: list[str]) -> str:
    return "\n".join(f"% {ln}" if ln else "%" for ln in lines)


def rust_crate_path(rel: Path) -> str:
    parts = list(rel.parts)
    comp = parts[0]

    if is_api_leaf(rel):
        i = parts.index("api")
        api_folder = parts[i - 1]
        prefix = "/".join(parts[: i - 1])
        return f"{OS_COMPONENTS}/{prefix}/{api_folder}/api-v0"

    if is_impl_leaf(rel):
        variant = rel.stem
        i = parts.index("impl")
        impl_folder = parts[i - 1]
        prefix = "/".join(parts[: i - 1])
        if impl_folder.endswith("-impl"):
            if variant == "common":
                return f"{OS_COMPONENTS}/{prefix}/{impl_folder}/{variant}"
            return f"{OS_COMPONENTS}/{prefix}/{impl_folder}/impl-{variant}"
        return f"{OS_COMPONENTS}/{prefix}/{impl_folder}/{variant}"

    if is_single_crate_leaf(rel):
        return f"{OS_COMPONENTS}/{comp}/{rel.parent.name}"

    return f"{OS_COMPONENTS}/{'/'.join(parts[:-1])}"


def export_doc_stem(comp: str) -> str:
    return comp  # wateros-mm 等


def comments_for_component_file(comp: str, short: str) -> list[str]:
    hint = COMPONENT_HINTS.get(comp, "")
    doc = export_doc_stem(comp)
    lines = [
        f"【本文件写什么】一级组件 {comp}（§ 级聚合）",
        f"  - 组件职责与在内核中的位置：{hint}" if hint else f"  - 组件职责与在内核中的位置",
        f"  - 聚合 crate {OS_COMPONENTS}/{comp}/src/lib.rs 的 pub mod 树与对外导出面",
        "  - 子模块划分、与相邻组件的依赖边界（谁依赖谁、走哪条门面）",
        "  - 根 feature / 本组件 Cargo.toml feature 如何选用各 impl",
        "  - 1～2 段综述后，由下方 \\input 展开子模块；不在此逐条列举 api trait",
        "【事实来源】",
        f"  - {OS_COMPONENTS}/{comp}/src/lib.rs、Cargo.toml",
        f"  - docs/exports/public-api/{doc}.md",
        f"  - docs/exports/features/{doc}.md",
        f"  - os/feature-tree.txt 中 {comp} 相关段",
        "【不写什么】",
        "  - api-v0 契约细节 → 子目录 api/api-v0.tex",
        "  - 各 impl 算法/平台细节 → 子目录 impl/*.tex",
    ]
    return lines


def comments_for_submodule_file(comp: str, rel: Path, heading: str) -> list[str]:
    # rel like wateros-ipc/ipc-waitqueue/waitqueue.tex
    parts = rel.parts
    sub_dir = parts[1] if len(parts) > 1 else ""
    hint = SUBMODULE_HINTS.get(sub_dir, "")
    lines = [
        f"【本文件写什么】子模块 {sub_dir}（\\{heading} 级聚合）",
    ]
    if hint:
        lines.append(f"  - 职责摘要：{hint}")
    else:
        lines.append("  - 本子域职责、解决的问题、与兄弟子模块的边界")
    lines += [
        f"  - 子域聚合 lib.rs：{OS_COMPONENTS}/{comp}/{sub_dir}/src/lib.rs",
        "  - 如何重导出 api-v0、feature 下挂载哪些 impl",
        "  - 被谁依赖（syscall、vfs、main 启动链等）",
        "  - 短综述 + 下方 \\input 展开 api 与 impl 叶子",
        "【事实来源】",
        f"  - {OS_COMPONENTS}/{comp}/{sub_dir}/**",
        f"  - docs/exports/ 中与 {comp} 相关的 public-api / features 章节",
    ]
    return lines


def comments_for_api_leaf(comp: str, rel: Path) -> list[str]:
    crate = rust_crate_path(rel)
    parts = rel.parts
    sub = parts[1] if len(parts) > 2 else comp
    lines = [
        "【本文件写什么】api-v0 契约层（叶子，只写语义不写实现）",
        f"  - crate 路径：{crate}/src/",
        "  - 列出并说明：pub trait、核心类型、错误枚举、常量",
        "  - 每个公开 API 的前置条件、后置条件、线程/中断上下文限制",
        "  - 与 Linux/用户态约定的对齐点与 deliberate 差异",
        "  - v0 版本当前行为与后续可替换 extension 点",
        "【事实来源】",
        f"  - 上述 crate 下全部 .rs 源文件",
        f"  - docs/exports/public-api/ 中对应符号表（以源码为准）",
        "【不写什么】",
        "  - impl 内部数据结构、汇编、具体算法步骤",
    ]
    return lines


def impl_hint(rel: Path) -> str:
    variant = rel.stem
    path_s = str(rel)
    if "platform-arch" in path_s and variant == "loongarch64":
        return "LoongArch trap 入口、上下文保存/恢复、定时器与 SMP（含 trap.S）。"
    if "platform-arch" in path_s and variant == "riscv64":
        return "RISC-V trap、上下文切换、定时器与 SMP。"
    if "mm-impl" in path_s and variant in ("loongarch64", "sv39"):
        return SUBMODULE_HINTS.get(variant, "")
    if "driver-block" in path_s or "driver-network" in path_s:
        return SUBMODULE_HINTS.get(variant, "")
    return SUBMODULE_HINTS.get(variant, "")


def comments_for_impl_leaf(comp: str, rel: Path) -> list[str]:
    crate = rust_crate_path(rel)
    variant = rel.stem
    hint = impl_hint(rel)
    lines = [
        f"【本文件写什么】impl 实现层：{variant}",
    ]
    if hint:
        lines.append(f"  - 定位：{hint}")
    lines += [
        f"  - crate 路径：{crate}/src/",
        "  - 如何实现对应 api-v0 trait：关键类型、状态机、数据结构",
        "  - 算法或硬件交互流程（可用伪代码/流程图）",
        "  - 本 impl 启用的 Cargo [features] 与 arch feature（impl-riscv64 等）",
        "  - 与其它 impl 变体的差异、切换 feature 时的影响",
        "  - 性能/内存假设与已知限制",
        "【事实来源】",
        f"  - 上述 crate 源码与 Cargo.toml",
        f"  - os/feature-tree.txt 中指向本 impl 的 feature 链",
    ]
    return lines


def comments_for_leaf_crate(comp: str, rel: Path) -> list[str]:
    crate = rust_crate_path(rel)
    sub = rel.parent.name
    hint = SUBMODULE_HINTS.get(sub, "")
    lines = [
        f"【本文件写什么】独立子 crate（无 api/impl 目录树）：{sub}",
    ]
    if hint:
        lines.append(f"  - 职责：{hint}")
    lines += [
        f"  - 源码：{crate}/src/",
        "  - 对外暴露的函数/类型、在聚合 lib.rs 中的重导出方式",
        "  - 实现要点、feature（若有）、与其它子 crate 的边界",
        "【事实来源】",
        f"  - {crate}/**、父组件 src/lib.rs",
    ]
    return lines


SECTION_SHORTS = {
    "abi", "base", "cred", "driver", "fs", "ipc", "klog", "mm",
    "platform", "pseudo-shell", "runtime", "syscall", "task", "utils", "vfs",
}


def is_component_root(rel: Path) -> bool:
    parts = rel.parts
    return len(parts) == 2 and rel.stem in SECTION_SHORTS


def is_api_leaf(rel: Path) -> bool:
    return len(rel.parts) >= 2 and rel.name == "api-v0.tex" and "api" in rel.parts


def is_impl_leaf(rel: Path) -> bool:
    return len(rel.parts) >= 2 and rel.parts[-2] == "impl" and rel.suffix == ".tex"


def is_single_crate_leaf(rel: Path) -> bool:
    """base-config/base-config.tex、runtime-logging/logging.tex 等。"""
    if len(rel.parts) != 3:
        return False
    return rel.parent.name == rel.stem


def classify_components_tex(rel: Path) -> list[str] | None:
    parts = rel.parts
    comp = parts[0]

    if is_component_root(rel):
        return comments_for_component_file(comp, rel.stem)

    if is_api_leaf(rel):
        return comments_for_api_leaf(comp, rel)

    if is_impl_leaf(rel):
        return comments_for_impl_leaf(comp, rel)

    if is_single_crate_leaf(rel):
        return comments_for_leaf_crate(comp, rel)

    if rel.suffix == ".tex":
        return comments_for_submodule_file(comp, rel, "subsection")

    return None


STRUCT_RE = re.compile(
    r"^\\(?:chapter|section|subsection|subsubsection|input|include|begin|end)\b"
)


def strip_leading_latex_comments(text: str) -> str:
    """去掉文件开头连续的 % 注释行与空行，保留正文。"""
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        s = lines[i].strip()
        if s == "" or s.startswith("%"):
            i += 1
            continue
        break
    rest = "\n".join(lines[i:])
    return rest.lstrip("\n")


def write_with_latex_comments(path: Path, comment_lines: list[str]) -> None:
    existing = path.read_text(encoding="utf-8") if path.exists() else ""
    body = strip_leading_latex_comments(existing)
    out = block(comment_lines)
    if body:
        out += "\n\n" + body
    if not out.endswith("\n"):
        out += "\n"
    path.write_text(out, encoding="utf-8")


def process_file(path: Path) -> None:
    try:
        rel_th = path.relative_to(DOC_ROOT)
    except ValueError:
        return
    key = str(rel_th).replace("\\", "/")
    # chapters/chap01.tex → chap01.tex；components 下文件不在 CHAPTER_FILES
    chapter_key = key
    if key.startswith("chapters/"):
        chapter_key = key[len("chapters/") :]

    if chapter_key in CHAPTER_FILES:
        write_with_latex_comments(path, CHAPTER_FILES[chapter_key])
        return

    if not str(path).startswith(str(COMPONENTS)):
        return

    rel = path.relative_to(COMPONENTS)
    comment_lines = classify_components_tex(rel)
    if comment_lines is None:
        comment_lines = [f"【待补充】请根据路径 {rel} 补充本文件写作说明"]
    write_with_latex_comments(path, comment_lines)


def main() -> None:
    targets: list[Path] = []
    if (DOC_ROOT / "main.tex").exists():
        targets.append(DOC_ROOT / "main.tex")
    if FRONTMATTER.exists():
        targets.extend(FRONTMATTER.rglob("*.tex"))
    for pat in ("chap01.tex", "chap04.tex", "chap05.tex", "chap02/*.tex", "chap03/*.tex"):
        targets.extend(CHAPTERS.glob(pat))
    targets.extend(COMPONENTS.rglob("*.tex"))

    for p in sorted(set(targets)):
        process_file(p)
        print(p.relative_to(DOC_ROOT))

    print(f"\nUpdated {len(targets)} files.")


if __name__ == "__main__":
    main()
