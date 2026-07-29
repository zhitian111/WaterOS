# 结果记录格式

本目录保存 `known-issues` 任务的小型文本结果，不保存原始日志、内核、镜像或 overlay。

每份结果至少包含：

```text
task:
date:
kernel_commit:
user_submodule_commit:
architecture:
qemu_and_firmware:
image_sha256:
overlay:
commands:
result_markers:
first_failure:
raw_log_path:
raw_log_sha256:
```

性能任务另记录修改前后每一轮原始值、中位数、波动和单项消融结果。功能或完整性失败
须保留最小复现、实际 errno/日志和第一个根因，不得只写“未通过”。原始日志放在仓库
外，例如 `/tmp/wateros-results/<task>/`。
