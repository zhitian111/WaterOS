# mGBA 源码台账

| 字段 | 值 |
| --- | --- |
| 上游 | `https://github.com/mgba-emu/mgba` |
| 许可证 | MPL-2.0（已由仓库 `LICENSE` 复核） |
| 固定版本 | `0.10.5` |
| 固定 commit | `26b7884bc25a5933960f3cdcd98bac1ae14d42e2` |
| 获取日期 | 2026-08-13 |
| 本地路径 | `user/vendor/mgba/` |
| WaterOS 补丁路径 | `user/packages/mgba/patches/` |
| `LICENSE` SHA-256 | `fab3dd6bdab226f1c08630b1dd917e11fcb4ec5e1e020e2c16f83a0a13863e85` |

## 获取规则

1. 上游源码以 `user/vendor/mgba/` git submodule 固定；测试 ROM 仅以压缩包形式纳入 Git；
2. 用 detached commit 固定源码；
3. 记录 `git rev-parse HEAD` 和源码树 SHA-256；
4. 不直接编辑 vendor；任何 WaterOS 修改以可审查 patch 保存；
5. 构建必须保持离线：依赖只能使用上游内含或已显式 vendored 的内容。

## 待补充的构建事实

- 当前 CMake 版本和最小 feature 开关；
- core public API、帧执行入口、视频/音频/input callback；
- raw ROM 和 save path 的文件 API；
- riscv64 Linux 静态链接结果；
- WaterOS 与 Linux 的 syscall 差异及回归测试。
