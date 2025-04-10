sudo apt update                  # 更新软件源
sudo apt install cargo           # cargo 工具链
sudo apt install rustc           # rustc 编译器
sudo apt install build-essential # 编译工具链
sudo apt install pkg-config      # 依赖库管理工具
sudo apt install libssl-dev      # openssl 库
rustup component add clippy      # 代码审查工具
rustup component add rustfmt     # 代码格式化工具
rustup component add rust-analyzer   # 语言服务器
cargo new hello_world --bin      # 创建新项目
cd hello_world                   # 进入项目目录
cargo build                      # 编译项目
cargo run                        # 运行项目
echo "如果看到Hello, world!，说明rust安装成功！"
cd ..
rm -rf hello_world               # 删除项目
rm Cargo.*                      # 删除 cargo 配置文件
