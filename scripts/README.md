# 脚本执行须知
## 权限修改
当你把脚本拉取下来之后，需要先进行权限修改，使得脚本具有执行权限。
假设你的终端模拟器的工作路径为项目根目录，则执行以下命令：
```bash
chmod +x ./scripts/script_name.sh
```
其中，script_name.sh 是你要执行的脚本文件名。
如果你不想麻烦，可以直接使用通配符为所有脚本文件添加执行权限：
```bash
chmod +x ./scripts/*.sh
```
具体命令请根据终端模拟器实际工作目录修改。
## 执行脚本
因为技术力原因，有的脚本的执行是不可逆的，因此在执行脚本之前，请务必清楚你在做什么。
假设你的终端模拟器的工作路径为项目根目录，则执行以下命令：
```bash
./scripts/script_name.sh
```
其中，script_name.sh 是你要执行的脚本文件名。
具体命令请根据终端模拟器实际工作目录修改。
## 脚本列表和效果说明
(顺序不分先后)
- ./scripts/rustc_target_for_oscmp.sh
    用于查看当前rust编译器支持的本次比赛需要的目标架构
- ./scripts/rustc_target_tools_install.sh
    用于安装构建对应平台的程序的rust工具链
