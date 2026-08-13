# WaterOS Nano-X 图形资产

`wateros-waves.png` 是为 WaterOS Nano-X 桌面生成的原始背景图。它在构建
`microwindows` package 时由 `tools/prepare_assets.py` 转换成 Nano-X 无额外
解码依赖即可读取的 1280×800 PPM；同一脚本还会用确定性绘制代码生成启动栏
图标。

背景图使用 Codex 内置 `imagegen` 生成，提示词记录在项目提交说明中。图中不
包含第三方商标、文字或水印。运行时镜像只安装转换后的 PPM 文件。
