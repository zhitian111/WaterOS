SCRIPT_DIR="$(dirname "$(realpath "$0")")"
echo "以下为kernel-la文件的elf解析信息：\r\n\r\n"
readelf -a $SCRIPT_DIR/../kernel-la
echo "\r\n\r\n以下为kernel-rv文件符号表信息：\r\n\r\n"
readelf -a $SCRIPT_DIR/../kernel-rv
