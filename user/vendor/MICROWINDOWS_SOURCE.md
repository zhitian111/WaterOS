# Microwindows / Nano-X 源码来源

- 上游仓库：`https://github.com/ghaerr/microwindows`
- 锁定提交：`2108675308cf69a5c1c54b483e29e3c039f319be`
- 提交日期：2026-07-29
- 许可证：见 `microwindows/LICENSE`

`microwindows/` 由上述提交直接导出，不包含开发工作区中的未提交修改。
WaterOS 的 framebuffer、evdev 与交叉编译适配全部位于
`packages/microwindows/patches/`，构建器会在临时工作目录中应用它们。
