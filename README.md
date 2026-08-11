# WaterOS

WaterOS 是使用 Rust 语言从零编写的操作系统内核，面向操作系统竞赛在 QEMU 上完成 bring-up 与测例验收。目前支持 **RISC-V64+OpenSBI** 与 **LoongArch64+ virtPCI** 双组合；内核按组件组织，通过 `api-*` 与 `impl-*` 及 Cargo feature 组装。

## 初赛材料

初赛材料包含初赛技术文档，初赛PPT，初赛视频以及对应的初赛视频的讲解脚本。

- 初赛文档请点[这里]
- 初赛初赛PPT请点[这里]
- 初赛视频请点[这里]
- 初赛视频脚本请点[这里]

## 提交包结构

比赛导出仓库仅包含以下顶层内容：

```text
.
├── Makefile          # 编译内核，并将 kernel-rv、kernel-la 复制到根目录
├── os/               # 内核工程
├── docs/             # 技术方案 LaTeX 与初赛材料
└── LICENSE
```

| 路径 | 说明 |
|------|------|
| `os/` | 内核源码、`components/`、Makefile、QEMU 与构建脚本 |
| `docs/` | `main.tex`、各章 `chapters/`、编译脚本；含 `初赛文档.pdf`、`初赛PPT.pptx`、`初赛讲解稿.md` |

## 编译内核

提交包根目录：

```bash
make all              # 生成 ./kernel-rv 与 ./kernel-la
```

开发时在 `os/` 下操作：

```bash
cd os
make configure        # 导出 feature-tree.txt 和 config.conf
make kernel-rv
make kernel-la
make rv_qemu_run      # RISC-V QEMU 运行
make la_qemu_run      # LoongArch QEMU 运行
make check
```

如果需要切换组件，请在 `./os/config.conf` 进行修改，修改完成后执行：

```bash
make apply_features   # 执行前请先执行 make configure
```

## 脚本

内核相关脚本位于 `./os/scripts/`；使用说明与推荐入口见
[`docs/tools/scripts/README.md`](./docs/tools/scripts/README.md)。在 **绝大多数** 情况下，不需要手动
执行脚本，常用功能已由 `./os/Makefile` 包装。

## 技术文档

编译技术文档

```bash
cd docs
./scripts/build.bash    # 输出 build/main.pdf
```

## 环境依赖

- Rust nightly 及 target：`riscv64gc-unknown-none-elf`、`loongarch64-unknown-none`（可参考 `user/rust-toolchain.toml`）
- QEMU：`qemu-system-riscv64`、`qemu-system-loongarch64`
- xelatex： 如果需要做文档编译的话
## 许可证

本仓库使用 MIT License，详细请见 [LICENSE](./LICENSE)。
