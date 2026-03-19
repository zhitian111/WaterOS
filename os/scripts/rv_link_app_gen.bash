#!/bin/bash
SCRIPT_DIR=$(dirname "$(readlink -f "$0")")
source "$SCRIPT_DIR/source/console.bash"

BIN_DIR="../wateros_user_mode_program/bin/riscv"
ELF_DIR="../wateros_user_mode_program/elf/riscv"
OUTPUT_FILE="./src/riscv/assembly_code/link_app.asm"

# 优先使用 ELF 目录
TARGET_DIR="$ELF_DIR"
EXT="elf"
if [[ ! -d "$TARGET_DIR" ]] || [[ -z $(ls "$TARGET_DIR"/*.elf 2>/dev/null) ]]; then
  info "elf 目录无有效文件，改用 bin 目录"
  TARGET_DIR="$BIN_DIR"
  EXT="bin"
fi

if [[ ! -d "$TARGET_DIR" ]]; then
  error "目录 $TARGET_DIR 不存在" 1
fi

files=($(ls "$TARGET_DIR"/*.${EXT} 2>/dev/null | sort))
if [[ ${#files[@]} -eq 0 ]]; then
  warning "目录 $TARGET_DIR 下没有找到任何 *.${EXT} 文件"
  exit 0
fi

info "找到 ${#files[@]} 个 ${EXT} 文件，将生成 $OUTPUT_FILE"

trace "生成头部"
cat > "$OUTPUT_FILE" <<EOF
    .align 3
    .section .data
    .global _app_names
    .global _num_app
_num_app:
    .quad ${#files[@]}
EOF

trace "写入每个应用的 start 符号"
for f in "${files[@]}"; do
  name=$(basename "$f" .${EXT})
  echo "    .quad _${name}_start" >> "$OUTPUT_FILE"
done

last_name=$(basename "${files[-1]}" .${EXT})
echo "    .quad _${last_name}_end" >> "$OUTPUT_FILE"
echo "" >> "$OUTPUT_FILE"

trace "写入每个应用的内容段"
for f in "${files[@]}"; do
  name=$(basename "$f" .${EXT})
  cat >> "$OUTPUT_FILE" <<EOF
    .section .data
    .global _${name}_start
    .global _${name}_end
_${name}_start:
    .incbin "$f"
_${name}_end:

EOF
done

trace "写入 _app_names"
echo "_app_names:" >> "$OUTPUT_FILE"
for f in "${files[@]}"; do
  name=$(basename "$f" .${EXT})
  echo "    .string \"${name}\"" >> "$OUTPUT_FILE"
done

info "生成完成：$OUTPUT_FILE"