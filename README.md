# 最新更改
## 2025-5-13
1. 实现了设备树解析和virtio-mmio设备定位。
2. 补充了注释模板，在源代码中补充了注释。
3. 添加了多个debug使用的宏。
4. 完成了riscv架构下的用户态和内核态的切换。
5. 完成了用户态调用系统资源的测试。
6. 新增./vendor/文件夹，用于存放第三方库。
7. 更改了riscv的rust代码的目录结构。
# 任务安排（来自老师）
3Q1 文档写出来（xv6 starry 其它OS）md \
3Q2 启动基本裸机程序 ，并学会使用git、rust，printf\
3Q3 编写启动代码启动MMU，INTC，TIM，CACHE\
3Q4 基本的内核框架（几个Syscall，能运行最简单的bench其中之一）

4Q1 60%非文件系统调用（包括musllibc）\
4Q2 100%非文件系统调用（包括musllibc）\
4Q3 追加完善文件系统功能（移植ext4或fatfs）\
4Q4 机动

5Q1 机动\
5Q2 整合网络协议栈 - 网络驱动编写 - 抄Linux或者u-boot然后C/Rust直接接口\
5Q3 GUI编写与测试\
5Q4 收尾，后面如果还有时间奔着决赛去

---
# 项目编码约定
1. 项目使用Rust语言编写，Cargo构建，git版本管理。所有文件均使用UTF-8编码。
2. 对于每一个.rs文件，文件的名称应该为小写英文字母，多个单词使用减号'-'链接。
3. 对于变量名，应该使用蛇形命名法，即所有字母小写，多个单词使用下划线'_'连接。
4. 对于函数名，应该使用蛇形命名法，即每个单词均小写，多个单词使用下划线'_'连接。
5. 对于常量名，应该使用全大写字母，多个单词使用下划线'_'连接。
6. 关于类型，尽管Rust支持编译器类型推导，但还是应该在变量名、函数参数、函数返回值等地方明确指定类型。
7. 关于函数返回值，尽管Rust支持表达式作为返回值，但还是应该显式的标明return
8. 关于表达式，如果需要使用表达式，必须使用注释表明是否使用了其中的返回值，如果有返回值，这个返回值的类型以及被返回的值的意义，并在表达式返回值处使用注释标注。
9. 关于注释，注释应当使用简体中文，并在每一个被注释代码的上方，且和"//"之间留有空格。每一个函数、变量、常量、表达式、自定义类型在定义时都应该有注释。
10. 关于不可变变量的使用，如果一个变量确认不会被修改，则必须使用let声明，或者说，只有在需要修改变量时才可以返回去为该变量添加mut，任何变量在定义时应该都是不可变的。
11. 关于所有权转移的情况，必须使用注释标注此时哪个变量拥有所有权，哪个变量失去了所有权。
12. 关于指针，尽量不要使用多个指针指向同一个可变变量。如必须使用，必须在更改时使用注释进行标注。
13. 一行里最多有一个点（‘.’）调用，如果有连续多个点调用，则需要进行换行并且点应该在同一列。
14. 在定义宏的时候，如果宏里使用了某个模块内的方法或类型，则应该在宏定义处使用crate::模块名::方法名或crate::模块名::类型名来调用，以便于宏的调用者省略不必要的use语句。
15. 对于unsafe代码，必须在代码前加上unsafe，并在注释中说明原因。且必须在unsafe代码前加上注释，说明这段代码的作用。
16. 对于方法（包括函数和宏）的注释，应尽量详细，使用markdown语法的文档注释格式，格式类似于（具体格式请见项目内已有的注释）：
```rust
/**
# 方法简介
包括方法的名称、功能描述、处理流程、是否会涉及到此函数外的数据（如全局变量、文件）、
是否调用了其他方法、是否有前置依赖、是否会修改传入的参数。
如果有前置依赖，则需要说明依赖的模块、方法、参数等。
如果有修改传入的参数，则需要说明修改的位置、修改的原因、修改的后果等。
如果有调用其他方法，则需要说明调用的模块、方法、参数等。
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| arg1 | i32 | 参数1 | 非负整数 | 0 |
| arg2 | &str | 参数2 | 非空字符串 | "" |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| ret1 | i32 | 返回值1 | 非负整数 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 | // 这条一般都会有，所以默认附带
# 注意事项
- 特殊情况：如果有特殊情况，需要说明，如需要特殊处理的边界条件、特殊情况的处理方式等。
- 性能优化：如果有性能优化，需要说明，如使用了哪些优化方法、优化的效果如何、优化的代价如何等。
 */
```
为了方便写注释，可以复制以下模板，基于此模板，可以快速编写注释，仅需填充空行即可：
```rust
/**
# 方法简介
## 方法名称

## 功能描述

## 处理流程

## 涉及数据

## 链式调用

## 前置依赖

## 是否修改参数

# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |

# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |

# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
*/
17. 如果有未完成的部分，尽量不要提交到主分支，如果有需要提交到主分支的或涉及到合作完成一个部分的情况，必须在提交信息中注明，并在代码中添加注释//TODO以及将其命名前加上TODO_。
18. 对于结构体和枚举的命名，应当使用大驼峰命名法，即每个单词的首字母大写，多个单词使用下划线'_'连接。
19. 对于模块的命名，应当使用小写字母，多个单词使用下划线'_'连接。
# 更改信息历史记录
此模块是方便查询项目规范和配置文件的更改信息，以免@lick长期摆烂落后进度。
## 2025-3-24
项目设置了.rustfmt.toml文件，用于rust代码格式化，请将toolchain改为nightly版本
具体来说请执行：
```bash
rustup toolchain install nightly
rustup default nightly
```
## 2025-3-26
1. 项目新增了scripts目录，用于存放项目的脚本文件，目前添加了两个文件：
- ./scripts/rustc_target_for_oscmp.sh
    用于查看当前rust编译器支持的本次比赛需要的目标架构
- ./scripts/rustc_target_tools_install.sh
    用于安装构建对应平台的程序的rust工具链
2. 项目新增了Makefile文件，在已经安装好rust工具链的情况下，可以直接使用make all命令进行编译。
如果没有rustup，请安装rustup：
```bash
sudo apt install rustup
```
## 2025-3-30
1. 添加了.cargo目录，用于配置cargo构建时的链接器脚本。
2. 更改了项目src的目录结构，请参考以下目录结构：
```bash
src                             # 项目源码目录
├── asm                         # 汇编相关代码目录
│   ├── loongarch               # loongarch架构相关汇编代码目录
│   └── riscv                   # riscv架构相关汇编代码目录
│       └── entry.asm           # 入口点汇编代码
├── rust                        # rust源码目录
│   ├── loongarch               # loongarch架构相关rust代码目录
│   │   └── loongarch-main.rs   # loongarch架构入口点rust代码
│   ├── riscv                   # riscv架构相关rust代码目录
│   │   └── riscv-main.rs       # riscv架构入口点rust代码
│   └── share                   # 共享代码目录
│       ├── io                  # 输入输出相关代码目录
│       │   ├── mod.rs          # 输入输出模块声明，也得声明pub mod
│       │   └── stdout.rs       # 标准输出相关代码
│       └── lib.rs              # 共享库代码，用于模块声明，加文件夹名字
└── script                      # 脚本目录，存放链接器脚本或其他脚本文件
    ├── loongarch               # loongarch架构链接器脚本
    └── riscv                   # riscv架构链接器脚本
        └── riscv-link.ld       # riscv架构链接器脚本文件
```
3. 调整了Cargo.toml文件，添加了目录内的库的crate，现在可以通过
    ```rust
    warter-os::模块名::子模块名::方法/类型名;
    ```
    这样的结构来调用库中的方法和类型。
4. 添加了./scripts/linker_toolchain_install.sh脚本，用于安装链接器脚本。目前还不完善，仅包括riscv64架构的链接器脚本。
5. 添加了./scripts/test_in_qemu_riscv.sh脚本，用于在qemu上运行riscv架构的测试程序。
ps:关于qemu的安装，请参考[qemu的官网](https://www.qemu.org/)，另外也可以通过[赛题发布页](https://github.com/oscomp/testsuits-for-oskernel)提供的docker环境来运行该脚本。
## 2025-4-9
1. 更改编码命名规范，详见项目编码约定中关于函数名的说明和新增的说明。
2. 更新注释规范。
3. 新增系统调用以及系统调用号。
4. 新增向主分支提交代码的批处理脚本（自己的分支请自行更改脚本内的分支名称）。
## 2025-4-10
1. 使用obsidian软件进行架构文档编写，详见os-arch目录。
2. 确认系统调用号以及任务列表。
3. 清除仓库中的多于测试脚本文件。
# 附加信息
1. 如果你需要使用neovim在Ubuntu上进行Rust编程，可以参考[我的nvim配置文件](https://github.com/zhitian111/nvim-configs-of-zhitian111)--zhitian111
