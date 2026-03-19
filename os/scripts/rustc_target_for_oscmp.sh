echo "loongarch64平台：\r\n\r\n"
rustc --print target-list | grep loongarch64

echo "\r\n\r\n"
echo "risc-v平台：\r\n\r\n"
rustc --print target-list | grep riscv64gc
